import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import * as apiClient from '../api/client'
import { buildReview, buildReviewRun } from '../testing/factories'
import { useReviewMutations } from './useReviewMutations'

afterEach(() => {
  vi.restoreAllMocks()
})

function createReviewMutationHarness() {
  const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('tasks')
  const creatingReview = ref(true)
  const errorMessage = ref('')
  const followingUpReview = ref<ReturnType<typeof buildReview> | null>(null)
  const followingUpReviewId = ref<string | null>(null)
  const reviewPendingDeletion = ref<ReturnType<typeof buildReview> | null>(null)
  const cancelingReviewId = ref<string | null>(null)
  const saving = ref(false)

  const refreshAll = vi.fn(async () => undefined)
  const removeReview = vi.fn()
  const replaceSelectedReviewRuns = vi.fn()
  const selectReview = vi.fn()
  const setFriendlyError = vi.fn()
  const upsertLatestReviewRun = vi.fn()
  const upsertReviewSummary = vi.fn()
  const upsertSelectedReviewRun = vi.fn()

  return {
    creatingReview,
    currentPage,
    followingUpReview,
    followingUpReviewId,
    refreshAll,
    removeReview,
    replaceSelectedReviewRuns,
    reviewPendingDeletion,
    upsertReviewSummary,
    upsertSelectedReviewRun,
    mutations: useReviewMutations({
      cancelingReviewId,
      creatingReview,
      currentPage,
      errorMessage,
      followingUpReview,
      followingUpReviewId,
      refreshAll,
      removeReview,
      replaceSelectedReviewRuns,
      reviewPendingDeletion,
      saving,
      selectReview,
      setFriendlyError,
      upsertLatestReviewRun,
      upsertReviewSummary,
      upsertSelectedReviewRun,
    }),
  }
}

describe('useReviewMutations', () => {
  it('creates a review, focuses the reviews page, and seeds drawer history', async () => {
    const harness = createReviewMutationHarness()
    const review = buildReview()
    const run = buildReviewRun({ reviewId: review.id })
    vi.spyOn(apiClient, 'createReview').mockResolvedValue({ review, run })

    await harness.mutations.createReviewFromWeb({
      pullRequestUrl: review.pullRequestUrl,
      preferredTool: review.preferredTool,
      extraInstructions: 'Check queue regressions.',
    })

    expect(harness.creatingReview.value).toBe(false)
    expect(harness.currentPage.value).toBe('reviews')
    expect(harness.replaceSelectedReviewRuns).toHaveBeenCalledWith([run])
    expect(harness.upsertReviewSummary).toHaveBeenCalledWith(review, run)
    expect(harness.refreshAll).toHaveBeenCalledTimes(1)
  })

  it('submits a re-review request and keeps the selected review history warm', async () => {
    const harness = createReviewMutationHarness()
    const review = buildReview({
      id: 'review-123',
      updatedAt: '2026-03-26T12:00:00.000Z',
    })
    const run = buildReviewRun({
      reviewId: review.id,
      createdAt: '2026-03-26T12:30:00.000Z',
      updatedAt: '2026-03-26T12:30:00.000Z',
    })
    harness.followingUpReview.value = review
    vi.spyOn(apiClient, 'followUpReview').mockResolvedValue(run)

    await harness.mutations.submitReviewFollowUp({ request: 'Re-check the latest push.' })

    expect(harness.upsertReviewSummary).toHaveBeenCalledWith(
      {
        ...review,
        updatedAt: run.createdAt,
      },
      run,
    )
    expect(harness.upsertSelectedReviewRun).toHaveBeenCalledWith(run)
    expect(harness.followingUpReview.value).toBeNull()
    expect(harness.followingUpReviewId.value).toBeNull()
    expect(harness.refreshAll).toHaveBeenCalledTimes(1)
  })
})
