import { ref, type ComputedRef, type Ref } from 'vue'

import type {
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
} from '../types/task'
import { useReviewViewState } from './useReviewViewState'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface UseReviewsScreenControllerOptions {
  data: {
    canRequestReview: ComputedRef<boolean>
    defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
    remoteAgentSettings: Ref<RemoteAgentSettings | null>
    reviewRequestDisabledReason: ComputedRef<string | undefined>
    reviews: Ref<ReviewSummary[]>
    saving: Ref<boolean>
  }
  reviewRunBridge: {
    refreshAll: () => Promise<void>
    removeReview: (reviewId: string) => void
    replaceSelectedReviewRuns: (reviewRuns: ReviewRunRecord[]) => void
    upsertLatestReviewRun: (reviewId: string, latestRun: ReviewRunRecord) => void
    upsertReviewSummary: (review: ReviewRecord, latestRun?: ReviewRunRecord | null) => void
    upsertSelectedReviewRun: (run: ReviewRunRecord) => void
  }
  shell: {
    currentPage: Ref<AppPage>
    errorMessage: Ref<string>
    setFriendlyError: (error: unknown) => void
  }
}

/**
 * Owns review-screen state while leaving data persistence in shared bridges.
 *
 * The review workflow has enough local intent that borrowing each ref from the
 * shell makes the screen harder to understand than the actual review behavior.
 * This controller now creates the drawer and modal state itself, so App.vue
 * can treat reviews as one domain boundary instead of one more prop bag.
 */
export function useReviewsScreenController(options: UseReviewsScreenControllerOptions) {
  const cancelingReviewId = ref<string | null>(null)
  const followingUpReviewId = ref<string | null>(null)
  const selectedReviewRuns = ref<ReviewRunRecord[]>([])

  const creatingReview = ref(false)
  const followingUpReview = ref<ReviewRecord | null>(null)
  const reviewPendingDeletion = ref<ReviewRecord | null>(null)

  const viewState = useReviewViewState({
    currentPage: options.shell.currentPage,
    followingUpReview,
    reviews: options.data.reviews,
    selectedReviewRuns,
  })

  return {
    cancelingReviewId,
    canRequestReview: options.data.canRequestReview,
    closeReviewDrawer: viewState.closeReviewDrawer,
    creatingReview,
    currentPage: options.shell.currentPage,
    defaultRemoteAgentPreferredTool: options.data.defaultRemoteAgentPreferredTool,
    errorMessage: options.shell.errorMessage,
    followingUpReview,
    followingUpReviewId,
    isReviewDrawerOpen: viewState.isReviewDrawerOpen,
    refreshAll: options.reviewRunBridge.refreshAll,
    remoteAgentSettings: options.data.remoteAgentSettings,
    removeReview: options.reviewRunBridge.removeReview,
    replaceSelectedReviewRuns: options.reviewRunBridge.replaceSelectedReviewRuns,
    reviewPendingDeletion,
    reviewRequestDisabledReason: options.data.reviewRequestDisabledReason,
    reviews: options.data.reviews,
    saving: options.data.saving,
    selectedReview: viewState.selectedReview,
    selectedReviewCanCancel: viewState.selectedReviewCanCancel,
    selectedReviewCanReReview: viewState.selectedReviewCanReReview,
    selectedReviewId: viewState.selectedReviewId,
    selectedReviewLatestRun: viewState.selectedReviewLatestRun,
    selectedReviewRuns,
    selectReview: viewState.selectReview,
    setFriendlyError: options.shell.setFriendlyError,
    upsertLatestReviewRun: options.reviewRunBridge.upsertLatestReviewRun,
    upsertReviewSummary: options.reviewRunBridge.upsertReviewSummary,
    upsertSelectedReviewRun: options.reviewRunBridge.upsertSelectedReviewRun,
  }
}

export type ReviewsScreenController = ReturnType<typeof useReviewsScreenController>
