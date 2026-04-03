import { describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'

import { useReviewViewState } from './useReviewViewState'
import { buildReview, buildReviewRun, buildReviewSummary } from '../testing/factories'

describe('useReviewViewState', () => {
  it('loads review history for the active drawer and clears it when leaving the reviews page', async () => {
    const reviewSummary = buildReviewSummary()
    const loadSelectedReviewRunHistory = vi.fn(async () => undefined)
    const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('reviews')
    const selectedReviewRuns = ref([buildReviewRun({ reviewId: reviewSummary.review.id })])
    const followingUpReview = ref(buildReview({ id: reviewSummary.review.id }))

    const scope = effectScope()
    const state = scope.run(() =>
      useReviewViewState({
        currentPage,
        followingUpReview,
        loadSelectedReviewRunHistory,
        reviews: ref([reviewSummary]),
        selectedReviewRuns,
        setFriendlyError: vi.fn(),
      }),
    )

    if (!state) {
      throw new Error('Expected review view state')
    }

    state.selectReview(reviewSummary.review.id)
    await nextTick()

    expect(loadSelectedReviewRunHistory).toHaveBeenCalledTimes(1)

    currentPage.value = 'tasks'
    await nextTick()

    expect(state.isReviewDrawerOpen.value).toBe(false)
    expect(selectedReviewRuns.value).toEqual([])
    expect(followingUpReview.value).toBeNull()

    scope.stop()
  })
})
