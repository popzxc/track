import { describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'

import { useReviewViewState } from './useReviewViewState'
import { buildReview, buildReviewRun, buildReviewSummary } from '../testing/factories'

describe('useReviewViewState', () => {
  it('loads review history for the active drawer and clears it when leaving the reviews page', async () => {
    const reviewSummary = buildReviewSummary()
    const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('reviews')
    const selectedReviewRuns = ref([buildReviewRun({ reviewId: reviewSummary.review.id })])
    const followingUpReview = ref(buildReview({ id: reviewSummary.review.id }))

    const scope = effectScope()
    const state = scope.run(() =>
      useReviewViewState({
        currentPage,
        followingUpReview,
        reviews: ref([reviewSummary]),
        selectedReviewRuns,
      }),
    )

    if (!state) {
      throw new Error('Expected review view state')
    }

    state.selectReview(reviewSummary.review.id)
    await nextTick()

    currentPage.value = 'tasks'
    await nextTick()

    expect(state.isReviewDrawerOpen.value).toBe(false)
    expect(selectedReviewRuns.value).toEqual([])
    expect(followingUpReview.value).toBeNull()

    scope.stop()
  })
})
