import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import App from './App.vue'
import {
  buildDispatch,
  buildProject,
  buildReviewRun,
  buildReviewSummary,
  buildRemoteAgentSettings,
  buildRunRecord,
  buildTask,
} from './testing/factories'

interface MockJsonRoute {
  method?: string
  path: string
  status?: number
  body: unknown | ((request: { init?: RequestInit; method: string; path: string }) => unknown)
}

type MockJsonRequest = { init?: RequestInit; method: string; path: string }

function installFetchRoutes(routes: MockJsonRoute[]) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const requestUrl =
      typeof input === 'string'
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url
    const resolvedUrl = new URL(requestUrl, 'http://localhost')
    const method = (init?.method ?? 'GET').toUpperCase()
    const requestPath = `${resolvedUrl.pathname}${resolvedUrl.search}`
    const route = routes.find((candidate) => (candidate.method ?? 'GET').toUpperCase() === method && candidate.path === requestPath)
      ?? routes.find((candidate) => (candidate.method ?? 'GET').toUpperCase() === method && candidate.path === resolvedUrl.pathname)

    if (!route) {
      throw new Error(`Unexpected fetch request: ${method} ${resolvedUrl.pathname}${resolvedUrl.search}`)
    }

    const responseBody = typeof route.body === 'function'
      ? route.body({ init, method, path: requestPath })
      : route.body

    return new Response(JSON.stringify(responseBody), {
      status: route.status ?? 200,
      headers: {
        'content-type': 'application/json',
      },
    })
  })

  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

async function mountApp() {
  const wrapper = mount(App, {
    global: {
      stubs: {
        teleport: true,
      },
    },
  })
  await flushPromises()
  await flushPromises()
  return wrapper
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('App shell', () => {
  it('groups tasks by project and opens the task drawer from the queue', async () => {
    const projectATask = buildTask({
      id: 'project-a/open/20260323-120000-fix-queue-layout.md',
      project: 'project-a',
      description: 'Fix queue layout\n\n## Summary\nKeep project sections visible.',
    })
    const projectBTask = buildTask({
      id: 'project-b/open/20260323-120100-review-run-history.md',
      project: 'project-b',
      description: 'Review run history\n\n## Summary\nMake latest runs easy to spot.',
    })

    installFetchRoutes([
      {
        path: '/api/projects',
        body: {
          projects: [
            buildProject({ canonicalName: 'project-a' }),
            buildProject({ canonicalName: 'project-b' }),
          ],
        },
      },
      {
        path: '/api/tasks',
        body: {
          tasks: [projectBTask, projectATask],
        },
      },
      {
        path: '/api/reviews',
        body: {
          reviews: [],
        },
      },
      {
        path: '/api/dispatches',
        body: {
          dispatches: [
            buildRunRecord(
              { ...projectATask },
              {
                dispatchId: 'dispatch-queue',
                taskId: projectATask.id,
                project: projectATask.project,
              },
            ).dispatch,
          ],
        },
      },
      {
        path: '/api/runs?limit=200',
        body: {
          runs: [],
        },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings(),
      },
      {
        path: `/api/tasks/${encodeURIComponent(projectATask.id)}/runs`,
        body: {
          runs: [
            buildRunRecord(
              { ...projectATask },
              {
                dispatchId: 'dispatch-queue',
                taskId: projectATask.id,
                project: projectATask.project,
              },
            ),
          ],
        },
      },
    ])

    const wrapper = await mountApp()

    const groups = wrapper.findAll('[data-testid="task-group"]')
    expect(groups.map((group) => group.attributes('data-project'))).toEqual(['project-a', 'project-b'])

    await wrapper.get(`[data-task-id="${projectATask.id}"]`).trigger('click')
    await flushPromises()

    expect(wrapper.get(`[data-task-id="${projectATask.id}"]`).text()).toContain('Succeeded')
    expect(wrapper.get('[data-testid="task-drawer"]').text()).toContain('Fix queue layout')
    expect(wrapper.get('[data-testid="run-latest-badge"]').text()).toBe('Latest')
    expect(wrapper.get('[data-testid="drawer-primary-action"]').text()).toContain('Continue run')
  })

  it('surfaces dispatch failures as a user-visible error banner', async () => {
    const task = buildTask()

    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [task] },
      },
      {
        path: '/api/reviews',
        body: { reviews: [] },
      },
      {
        path: '/api/dispatches',
        body: { dispatches: [] },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings(),
      },
      {
        path: `/api/tasks/${encodeURIComponent(task.id)}/runs`,
        body: { runs: [] },
      },
      {
        method: 'POST',
        path: `/api/tasks/${task.id}/dispatch`,
        status: 502,
        body: {
          error: {
            code: 'REMOTE_DISPATCH_FAILED',
            message: 'Runner offline.',
          },
        },
      },
    ])

    const wrapper = await mountApp()

    await wrapper.get(`[data-task-id="${task.id}"]`).trigger('click')
    await flushPromises()
    await wrapper.get('[data-testid="drawer-primary-action"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="error-banner"]').text()).toContain('Runner offline.')
  })

  it('updates the visible run state immediately after a successful dispatch response', async () => {
    const task = buildTask()

    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [task] },
      },
      {
        path: '/api/reviews',
        body: { reviews: [] },
      },
      {
        path: '/api/dispatches',
        body: { dispatches: [] },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings(),
      },
      {
        path: `/api/tasks/${encodeURIComponent(task.id)}/runs`,
        body: { runs: [] },
      },
      {
        method: 'POST',
        path: `/api/tasks/${task.id}/dispatch`,
        body: buildDispatch({
          dispatchId: 'dispatch-started',
          taskId: task.id,
          project: task.project,
          status: 'running',
          summary: 'The remote agent is working in the prepared environment.',
        }),
      },
    ])

    const wrapper = await mountApp()

    await wrapper.get(`[data-task-id="${task.id}"]`).trigger('click')
    await flushPromises()
    await wrapper.get('[data-testid="drawer-primary-action"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="run-history-item"]').text()).toContain('Agent running')
  })

  it('gates manual PR reviews until a main GitHub user is configured', async () => {
    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [] },
      },
      {
        path: '/api/reviews',
        body: { reviews: [] },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings({
          reviewFollowUp: {
            enabled: false,
          },
        }),
      },
    ])

    const wrapper = await mountApp()

    const reviewsButton = wrapper.findAll('button').find((button) => button.text().includes('Reviews'))
    await reviewsButton?.trigger('click')
    await flushPromises()

    const requestReviewButton = wrapper.findAll('button').find((button) => button.text().includes('Request review'))
    expect(requestReviewButton?.attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('Set the main GitHub user in Settings to enable PR reviews.')
  })

  it('creates a review request, opens the review drawer, and deletes it cleanly', async () => {
    const createdReview = buildReviewSummary()
    let reviewsBody: { reviews: unknown[] } = { reviews: [] }
    let submittedReviewRequest: Record<string, unknown> | null = null

    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [] },
      },
      {
        path: '/api/reviews',
        body: () => reviewsBody,
      },
      {
        path: `/api/reviews/${encodeURIComponent(createdReview.review.id)}/runs`,
        body: {
          runs: [buildReviewRun({
            reviewId: createdReview.review.id,
            pullRequestUrl: createdReview.review.pullRequestUrl,
          })],
        },
      },
      {
        method: 'POST',
        path: '/api/reviews',
        body: ({ init }: MockJsonRequest) => {
          submittedReviewRequest = JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>
          reviewsBody = { reviews: [createdReview] }

          return {
            review: createdReview.review,
            run: createdReview.latestRun,
          }
        },
      },
      {
        method: 'DELETE',
        path: `/api/reviews/${createdReview.review.id}`,
        body: () => {
          reviewsBody = { reviews: [] }
          return { ok: true }
        },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings({
          reviewFollowUp: {
            enabled: false,
            mainUser: 'octocat',
            defaultReviewPrompt: 'Focus on risky behavior changes and missing tests.',
          },
        }),
      },
    ])

    const wrapper = await mountApp()

    const reviewsButton = wrapper.findAll('button').find((button) => button.text().includes('Reviews'))
    await reviewsButton?.trigger('click')
    await flushPromises()

    const requestReviewButton = wrapper.findAll('button').find((button) => button.text().includes('Request review'))
    await requestReviewButton?.trigger('click')
    await flushPromises()

    await wrapper.get('[data-testid="review-request-url"]').setValue(createdReview.review.pullRequestUrl)
    await wrapper.get('[data-testid="review-request-extra-instructions"]').setValue('Pay attention to queue regressions.')
    await wrapper.get('[data-testid="review-request-submit"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect(submittedReviewRequest).toEqual({
      pullRequestUrl: createdReview.review.pullRequestUrl,
      extraInstructions: 'Pay attention to queue regressions.',
    })
    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain(createdReview.review.pullRequestTitle)
    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain('Submitted a GitHub review with two inline comments.')

    const deleteReviewButton = wrapper.findAll('button').find((button) => button.text().includes('Delete review'))
    await deleteReviewButton?.trigger('click')
    await flushPromises()
    await wrapper.get('[data-testid="confirm-submit"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect(wrapper.find('[data-testid="review-drawer"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('No PR reviews yet.')
  })

  it('requests a re-review on the same saved review and keeps the new run in history', async () => {
    const reviewSummary = buildReviewSummary()
    const initialRun = buildReviewRun({
      reviewId: reviewSummary.review.id,
      pullRequestUrl: reviewSummary.review.pullRequestUrl,
      targetHeadOid: 'abc123def456',
    })
    const followUpRun = buildReviewRun({
      dispatchId: 'review-dispatch-456',
      reviewId: reviewSummary.review.id,
      pullRequestUrl: reviewSummary.review.pullRequestUrl,
      createdAt: '2026-03-26T13:05:00.000Z',
      updatedAt: '2026-03-26T13:06:00.000Z',
      finishedAt: '2026-03-26T13:06:00.000Z',
      followUpRequest: 'Check whether the comments I confirmed are fixed.',
      targetHeadOid: 'def456abc789',
      summary: 'Submitted a follow-up review after checking the latest PR updates.',
      githubReviewId: '1002',
      githubReviewUrl: 'https://github.com/acme/project-a/pull/42#pullrequestreview-1002',
    })
    let reviewRuns = [initialRun]
    let reviewsBody = { reviews: [{ review: reviewSummary.review, latestRun: initialRun }] }
    let submittedReviewFollowUp: Record<string, unknown> | null = null

    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [] },
      },
      {
        path: '/api/reviews',
        body: () => reviewsBody,
      },
      {
        path: `/api/reviews/${encodeURIComponent(reviewSummary.review.id)}/runs`,
        body: () => ({ runs: reviewRuns }),
      },
      {
        method: 'POST',
        path: `/api/reviews/${encodeURIComponent(reviewSummary.review.id)}/follow-up`,
        body: ({ init }: MockJsonRequest) => {
          submittedReviewFollowUp = JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>
          reviewRuns = [followUpRun, initialRun]
          reviewsBody = {
            reviews: [
              {
                review: {
                  ...reviewSummary.review,
                  updatedAt: followUpRun.createdAt,
                },
                latestRun: followUpRun,
              },
            ],
          }
          return followUpRun
        },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings({
          reviewFollowUp: {
            enabled: false,
            mainUser: 'octocat',
            defaultReviewPrompt: 'Focus on risky behavior changes and missing tests.',
          },
        }),
      },
    ])

    const wrapper = await mountApp()

    const reviewsButton = wrapper.findAll('button').find((button) => button.text().includes('Reviews'))
    await reviewsButton?.trigger('click')
    await flushPromises()

    await wrapper.get('[data-testid="review-row"]').trigger('click')
    await flushPromises()

    const rereviewButton = wrapper.findAll('button').find((button) => button.text().includes('Request re-review'))
    await rereviewButton?.trigger('click')
    await flushPromises()

    await wrapper.get('[data-testid="review-follow-up-request"]').setValue(
      'Check whether the comments I confirmed are fixed.',
    )
    await wrapper.get('[data-testid="review-follow-up-submit"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect(submittedReviewFollowUp).toEqual({
      request: 'Check whether the comments I confirmed are fixed.',
    })
    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain('Request re-review')
    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain('Pinned commit')
    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain('def456abc789')
    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain('Re-review request')
    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain(
      'Check whether the comments I confirmed are fixed.',
    )
  })

  it('shows active PR reviews on the Runs page and in the Runs badge', async () => {
    const runningReview = buildReviewSummary({
      latestRun: {
        status: 'running',
        summary: 'Reviewing the pull request remotely.',
        reviewSubmitted: false,
      },
    })

    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [] },
      },
      {
        path: '/api/reviews',
        body: { reviews: [runningReview] },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings({
          reviewFollowUp: {
            enabled: false,
            mainUser: 'octocat',
          },
        }),
      },
    ])

    const wrapper = await mountApp()

    const runsButton = wrapper.findAll('button').find((button) => button.text().includes('Runs'))
    expect(runsButton?.text()).toContain('1')

    await runsButton?.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Active PR reviews')
    expect(wrapper.text()).toContain(runningReview.review.pullRequestTitle)
    expect(wrapper.text()).toContain('Open review')
  })
})
