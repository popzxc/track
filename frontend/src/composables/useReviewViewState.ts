import { computed, ref, watch, type Ref } from 'vue'

import type { ReviewRecord, ReviewRunRecord, ReviewSummary } from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface UseReviewViewStateOptions {
  currentPage: Ref<AppPage>
  followingUpReview: Ref<ReviewRecord | null>
  reviews: Ref<ReviewSummary[]>
  selectedReviewRuns: Ref<ReviewRunRecord[]>
}

/**
 * Keeps the review list, review drawer, and re-review modal synchronized.
 *
 * Review state is smaller than task state, but it still has the same
 * lifecycle-sensitive behavior: the drawer should close when the selected
 * review disappears. Pulling that coordination into one composable makes the
 * remaining shell code read as review mutations instead of review bookkeeping.
 */
export function useReviewViewState(options: UseReviewViewStateOptions) {
  const selectedReviewId = ref<string | null>(null)
  const isReviewDrawerOpen = ref(false)

  const selectedReviewSummary = computed(() =>
    options.reviews.value.find((summary) => summary.review.id === selectedReviewId.value) ?? null,
  )

  const selectedReview = computed(() => selectedReviewSummary.value?.review ?? null)

  const selectedReviewLatestRun = computed(() => selectedReviewSummary.value?.latestRun ?? null)

  const selectedReviewCanCancel = computed(() =>
    Boolean(
      selectedReview.value &&
        selectedReviewLatestRun.value &&
        (
          selectedReviewLatestRun.value.status === 'preparing' ||
          selectedReviewLatestRun.value.status === 'running'
        ),
    ),
  )

  const selectedReviewCanReReview = computed(() =>
    Boolean(selectedReview.value && !selectedReviewCanCancel.value),
  )

  function selectReview(reviewId: string) {
    selectedReviewId.value = reviewId
    isReviewDrawerOpen.value = true

    if (options.currentPage.value !== 'reviews') {
      options.currentPage.value = 'reviews'
    }
  }

  function closeReviewDrawer() {
    isReviewDrawerOpen.value = false
    selectedReviewId.value = null
    options.selectedReviewRuns.value = []
    options.followingUpReview.value = null
  }

  watch(
    options.reviews,
    (nextReviews) => {
      if (
        selectedReviewId.value &&
        !nextReviews.some((summary) => summary.review.id === selectedReviewId.value)
      ) {
        closeReviewDrawer()
      }
    },
    { immediate: true },
  )

  watch(options.currentPage, (nextPage) => {
    if (nextPage !== 'reviews') {
      isReviewDrawerOpen.value = false
      options.selectedReviewRuns.value = []
      options.followingUpReview.value = null
    }
  })

  return {
    closeReviewDrawer,
    isReviewDrawerOpen,
    selectReview,
    selectedReview,
    selectedReviewCanCancel,
    selectedReviewCanReReview,
    selectedReviewId,
    selectedReviewLatestRun,
    selectedReviewSummary,
  }
}
