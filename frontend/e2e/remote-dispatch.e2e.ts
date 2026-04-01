import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { chromium, type Browser, type Page } from '@playwright/test'

import {
  DISPATCH_TASK_TITLE,
  E2E_PR_URL,
  FOLLOW_UP_TASK_TITLE,
  FIXTURE_HOST,
  FIXTURE_USER,
  ORPHAN_CLEANUP_DISPATCH_ID,
  ORPHAN_CLEANUP_TASK_ID,
  E2E_PROJECT_NAME,
  E2E_REVIEW_TITLE,
  FIXTURE_WORKSPACE_ROOT,
} from '../../testing/e2e/support/constants'
import { loadFrontendE2EState } from '../../testing/e2e/support/state'
import { setupFrontendE2EEnvironment } from '../../testing/e2e/global.setup'
import { teardownFrontendE2EEnvironment } from '../../testing/e2e/global.teardown'

let browser: Browser

// =============================================================================
// Sparse Browser Smoke Path
// =============================================================================
//
// These tests are intentionally expensive, so we keep them focused on one
// end-to-end story: can the user dispatch and then continue agent work through
// the real browser UI while the backend talks to the real SSH fixture?
beforeAll(async () => {
  await setupFrontendE2EEnvironment()
  browser = await chromium.launch({
    headless: true,
  })
})

afterAll(async () => {
  await browser?.close()
  await teardownFrontendE2EEnvironment()
})

async function openTaskDrawer(page: Page, taskTitle: string) {
  const taskRow = page.getByTestId('task-row').filter({ hasText: taskTitle }).first()
  await taskRow.click()
  await page.getByTestId('task-drawer').waitFor()

  const drawerText = await page.getByTestId('task-drawer').textContent()
  expect(drawerText).toContain(taskTitle)
}

async function openReviewDrawer(page: Page, reviewTitle: string) {
  await page.getByRole('button', { name: 'Reviews' }).click()
  const reviewRow = page.getByTestId('review-row').filter({ hasText: reviewTitle }).first()
  await reviewRow.click()
  await page.getByTestId('review-drawer').waitFor()

  const drawerText = await page.getByTestId('review-drawer').textContent()
  expect(drawerText).toContain(reviewTitle)
}

// =============================================================================
// Stable UI Polling Helpers
// =============================================================================
//
// The browser smoke tests drive the real API and the real SSH fixture, so
// dispatch completion time can vary a bit even when the mocked Codex outcome is
// deterministic. Rather than baking in brittle fixed sleeps, we reload the page
// until the drawer shows the terminal label we care about.
async function waitForRunHistoryLabel(
  page: Page,
  taskTitle: string,
  expectedLabel: string,
  timeoutMs: number,
) {
  const deadline = Date.now() + timeoutMs
  let lastRunHistoryText = ''

  while (Date.now() < deadline) {
    await page.reload()
    await openTaskDrawer(page, taskTitle)
    lastRunHistoryText = (await page.getByTestId('run-history-item').first().textContent()) ?? ''
    if (lastRunHistoryText.includes(expectedLabel)) {
      return
    }

    await page.waitForTimeout(1_000)
  }

  throw new Error(
    `Timed out waiting for "${expectedLabel}" in the run history for "${taskTitle}". Last visible run history entry:\n${lastRunHistoryText}`,
  )
}

// Wait for the drawer's primary action button to show the expected text.
//
// The button label depends on selectedTaskLatestDispatch, which is loaded by
// loadLatestDispatchesForVisibleTasks() — a separate fetch from the run-history
// data that waitForRunHistoryLabel reads. When waitForRunHistoryLabel returns it
// may have caught the run-history update from a watch-triggered fetch that
// completed slightly before the refreshAll() second Promise.all finished. In
// that case selectedTaskLatestDispatch still has stale data and the button shows
// the wrong label. Reloading gives the page a full fresh cycle so both fetches
// complete in the same Promise.all, and the button is guaranteed to reflect the
// final dispatch state.
async function waitForDrawerPrimaryAction(
  page: Page,
  taskTitle: string,
  expectedText: string,
  timeoutMs: number,
) {
  const deadline = Date.now() + timeoutMs

  while (Date.now() < deadline) {
    const remaining = deadline - Date.now()
    try {
      await page
        .getByTestId('drawer-primary-action')
        .filter({ hasText: expectedText })
        .waitFor({ timeout: Math.min(5_000, remaining) })
      return
    } catch {
      if (Date.now() >= deadline) {
        break
      }
      // The button hasn't updated yet on the current page load. Reload so that
      // all data fetches (including loadLatestDispatchesForVisibleTasks) run in
      // a single coordinated refreshAll() pass.
      await page.reload()
      await openTaskDrawer(page, taskTitle)
    }
  }

  const currentText = await page.getByTestId('drawer-primary-action').textContent()
  throw new Error(
    `Timed out waiting for drawer-primary-action with text "${expectedText}" for "${taskTitle}". ` +
    `Current button text: "${currentText?.trim()}"`,
  )
}

async function waitForReviewRunLabel(
  page: Page,
  reviewTitle: string,
  expectedLabel: string,
  timeoutMs: number,
) {
  const deadline = Date.now() + timeoutMs
  let lastDrawerText = ''

  while (Date.now() < deadline) {
    await page.reload()
    await openReviewDrawer(page, reviewTitle)
    lastDrawerText = (await page.getByTestId('review-drawer').textContent()) ?? ''
    if (lastDrawerText.includes(expectedLabel)) {
      return lastDrawerText
    }

    await page.waitForTimeout(1_000)
  }

  throw new Error(
    `Timed out waiting for "${expectedLabel}" in the review drawer for "${reviewTitle}". Last visible review drawer contents:\n${lastDrawerText}`,
  )
}

function orphanWorktreePath() {
  return `${FIXTURE_WORKSPACE_ROOT}/${E2E_PROJECT_NAME}/worktrees/${ORPHAN_CLEANUP_DISPATCH_ID}`
}

function orphanRunDirectory() {
  return `${FIXTURE_WORKSPACE_ROOT}/${E2E_PROJECT_NAME}/dispatches/${ORPHAN_CLEANUP_DISPATCH_ID}`
}

function remotePathExists(remotePath: string): boolean {
  const state = loadFrontendE2EState()
  const result = spawnSync(
    'ssh',
    [
      '-i',
      path.join(state.tempRoot, 'fixture-key', 'id_ed25519'),
      '-p',
      String(state.fixturePort),
      '-o',
      'BatchMode=yes',
      '-o',
      'IdentitiesOnly=yes',
      '-o',
      'StrictHostKeyChecking=accept-new',
      '-o',
      `UserKnownHostsFile=${path.join(state.runtimeRoot, 'known_hosts')}`,
      `${FIXTURE_USER}@${FIXTURE_HOST}`,
      'test',
      '-e',
      remotePath,
    ],
    {
      encoding: 'utf-8',
    },
  )

  return result.status === 0
}

describe('remote dispatch smoke flow', () => {
  test('requests a PR review from the UI and deletes it after completion', async () => {
    const { apiBaseUrl } = loadFrontendE2EState()
    const page = await browser.newPage()

    try {
      await page.goto(apiBaseUrl)
      await page.getByRole('button', { name: 'Reviews' }).click()
      await page.getByRole('button', { name: 'Request review' }).click()
      await page.getByTestId('review-request-modal').waitFor()
      await page.getByTestId('review-request-url').fill(E2E_PR_URL)
      await page.getByTestId('review-request-extra-instructions').fill('Pay extra attention to regression coverage.')
      await page.getByTestId('review-request-submit').click()
      await page.getByTestId('review-drawer').waitFor()

      const drawerText = await waitForReviewRunLabel(page, E2E_REVIEW_TITLE, 'Succeeded', 20_000)
      expect(drawerText).toContain('Review submitted')

      await page.getByRole('button', { name: 'Delete review' }).click()
      await page.getByTestId('confirm-dialog').waitFor()
      await page.getByTestId('confirm-submit').click()
      await page.waitForTimeout(1_000)

      await page.reload()
      await page.getByRole('button', { name: 'Reviews' }).click()
      expect(await page.getByTestId('review-row').count()).toBe(0)
    } finally {
      await page.close()
    }
  }, 120_000)

  test('dispatches from the UI and continues with a follow-up request', async () => {
    const { apiBaseUrl } = loadFrontendE2EState()
    const page = await browser.newPage()

    try {
      await page.goto(apiBaseUrl)

      await openTaskDrawer(page, DISPATCH_TASK_TITLE)
      await page.getByTestId('drawer-primary-action').click()
      await page.getByTestId('run-history-item').first().waitFor()

      const firstDispatchText = await page.getByTestId('run-history-item').first().textContent()
      expect(firstDispatchText).toContain('Preparing environment')

      await waitForRunHistoryLabel(page, DISPATCH_TASK_TITLE, 'Succeeded', 20_000)
      const latestDispatchText = await page.getByTestId('run-history-item').first().textContent()
      expect(latestDispatchText).toContain('Succeeded')
      expect(await page.getByRole('button', { name: 'View PR' }).count()).toBeGreaterThan(0)

      await page.reload()
      await openTaskDrawer(page, FOLLOW_UP_TASK_TITLE)
      await page.getByTestId('drawer-primary-action').click()
      await page.getByTestId('run-history-item').first().waitFor()
      await waitForRunHistoryLabel(page, FOLLOW_UP_TASK_TITLE, 'Succeeded', 20_000)
      const followUpReadyText = await page.getByTestId('run-history-item').first().textContent()
      expect(followUpReadyText).toContain('Succeeded')

      // Wait for the primary action button to switch to "Continue run" before
      // clicking. The button uses selectedTaskLatestDispatch.branchName which
      // requires loadLatestDispatchesForVisibleTasks() to have completed with
      // the final dispatch data. That fetch runs in a different code path from
      // the run-history fetch, so a reload may be needed to guarantee both
      // settle together. 45 s is enough time for multiple reload cycles on a
      // slow runner; reaching it would mean the API is not returning a completed
      // dispatch with branchName/worktreePath, which is a genuine bug.
      await waitForDrawerPrimaryAction(page, FOLLOW_UP_TASK_TITLE, 'Continue run', 45_000)
      await page.getByTestId('drawer-primary-action').click()
      await page.getByTestId('follow-up-modal').waitFor({ timeout: 30_000 })
      await page.getByTestId('follow-up-request').fill('Address the review comments on the open PR.')
      await page.getByTestId('follow-up-submit').click()
      await waitForRunHistoryLabel(page, FOLLOW_UP_TASK_TITLE, 'Follow-up', 20_000)
      const runHistoryItems = page.getByTestId('run-history-item')
      expect(await runHistoryItems.count()).toBe(2)
      expect(await runHistoryItems.first().textContent()).toContain('Latest')
      expect(await runHistoryItems.first().textContent()).toContain('Follow-up')
      expect(await page.getByRole('button', { name: 'View PR' }).count()).toBeGreaterThan(0)
    } finally {
      await page.close()
    }
  }, 120_000)

  test('cleans up orphaned remote artifacts from Settings', async () => {
    const { apiBaseUrl } = loadFrontendE2EState()
    const page = await browser.newPage()

    try {
      expect(remotePathExists(orphanWorktreePath())).toBe(true)
      expect(remotePathExists(orphanRunDirectory())).toBe(true)

      await page.goto(apiBaseUrl)
      await page.getByRole('button', { name: 'Settings' }).click()
      await page.getByTestId('settings-cleanup-button').click()
      await page.getByTestId('confirm-dialog').waitFor()
      await page.getByTestId('confirm-submit').click()
      await page.getByTestId('cleanup-summary').waitFor()

      const cleanupSummaryText = await page.getByTestId('cleanup-summary').textContent()
      expect(cleanupSummaryText).toContain('Missing tasks')
      expect(cleanupSummaryText).toContain('Local histories')

      expect(remotePathExists(orphanWorktreePath())).toBe(false)
      expect(remotePathExists(orphanRunDirectory())).toBe(false)
    } finally {
      await page.close()
    }
  }, 120_000)
})
