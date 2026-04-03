import type { ComputedRef, Ref } from 'vue'

import type {
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
} from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface ReviewViewState {
  closeReviewDrawer: () => void
  isReviewDrawerOpen: Ref<boolean>
  selectReview: (reviewId: string) => void
  selectedReview: ComputedRef<ReviewRecord | null>
  selectedReviewCanCancel: ComputedRef<boolean>
  selectedReviewCanReReview: ComputedRef<boolean>
  selectedReviewLatestRun: ComputedRef<ReviewRunRecord | null>
}

interface UseReviewsScreenControllerOptions {
  data: {
    canRequestReview: ComputedRef<boolean>
    defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
    remoteAgentSettings: Ref<RemoteAgentSettings | null>
    reviewRequestDisabledReason: ComputedRef<string | undefined>
    reviews: Ref<ReviewSummary[]>
    saving: Ref<boolean>
    selectedReviewRuns: Ref<ReviewRunRecord[]>
  }
  overlays: {
    creatingReview: Ref<boolean>
    followingUpReview: Ref<ReviewRecord | null>
    reviewPendingDeletion: Ref<ReviewRecord | null>
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
  viewState: ReviewViewState
  workflow: {
    cancelingReviewId: Ref<string | null>
    followingUpReviewId: Ref<string | null>
  }
}

/**
 * Defines the review screen's dependency boundary in one place.
 *
 * The review workflow now has its own screen, but App.vue still owns the raw
 * refs. This controller keeps the composition readable by naming the review
 * surface explicitly while the deeper ownership split remains a future step.
 */
export function useReviewsScreenController(options: UseReviewsScreenControllerOptions) {
  return {
    cancelingReviewId: options.workflow.cancelingReviewId,
    canRequestReview: options.data.canRequestReview,
    closeReviewDrawer: options.viewState.closeReviewDrawer,
    creatingReview: options.overlays.creatingReview,
    currentPage: options.shell.currentPage,
    defaultRemoteAgentPreferredTool: options.data.defaultRemoteAgentPreferredTool,
    errorMessage: options.shell.errorMessage,
    followingUpReview: options.overlays.followingUpReview,
    followingUpReviewId: options.workflow.followingUpReviewId,
    isReviewDrawerOpen: options.viewState.isReviewDrawerOpen,
    refreshAll: options.reviewRunBridge.refreshAll,
    remoteAgentSettings: options.data.remoteAgentSettings,
    removeReview: options.reviewRunBridge.removeReview,
    replaceSelectedReviewRuns: options.reviewRunBridge.replaceSelectedReviewRuns,
    reviewPendingDeletion: options.overlays.reviewPendingDeletion,
    reviewRequestDisabledReason: options.data.reviewRequestDisabledReason,
    reviews: options.data.reviews,
    saving: options.data.saving,
    selectedReview: options.viewState.selectedReview,
    selectedReviewCanCancel: options.viewState.selectedReviewCanCancel,
    selectedReviewCanReReview: options.viewState.selectedReviewCanReReview,
    selectedReviewLatestRun: options.viewState.selectedReviewLatestRun,
    selectedReviewRuns: options.data.selectedReviewRuns,
    selectReview: options.viewState.selectReview,
    setFriendlyError: options.shell.setFriendlyError,
    upsertLatestReviewRun: options.reviewRunBridge.upsertLatestReviewRun,
    upsertReviewSummary: options.reviewRunBridge.upsertReviewSummary,
    upsertSelectedReviewRun: options.reviewRunBridge.upsertSelectedReviewRun,
  }
}

export type ReviewsScreenController = ReturnType<typeof useReviewsScreenController>
