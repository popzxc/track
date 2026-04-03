<script setup lang="ts">
import type { ComputedRef, Ref } from 'vue'

import ConfirmDialog from './ConfirmDialog.vue'
import ReviewDrawer from './ReviewDrawer.vue'
import ReviewFollowUpModal from './ReviewFollowUpModal.vue'
import ReviewRequestModal from './ReviewRequestModal.vue'
import ReviewsPage from './ReviewsPage.vue'
import { useReviewMutations } from '../composables/useReviewMutations'
import type {
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
} from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface ReviewsScreenContext {
  cancelingReviewId: Ref<string | null>
  canRequestReview: ComputedRef<boolean>
  closeReviewDrawer: () => void
  creatingReview: Ref<boolean>
  currentPage: Ref<AppPage>
  defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
  errorMessage: Ref<string>
  followingUpReview: Ref<ReviewRecord | null>
  followingUpReviewId: Ref<string | null>
  isReviewDrawerOpen: Ref<boolean>
  refreshAll: () => Promise<void>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  removeReview: (reviewId: string) => void
  replaceSelectedReviewRuns: (reviewRuns: ReviewRunRecord[]) => void
  reviewPendingDeletion: Ref<ReviewRecord | null>
  reviewRequestDisabledReason: ComputedRef<string | undefined>
  reviews: Ref<ReviewSummary[]>
  saving: Ref<boolean>
  selectedReview: ComputedRef<ReviewRecord | null>
  selectedReviewCanCancel: ComputedRef<boolean>
  selectedReviewCanReReview: ComputedRef<boolean>
  selectedReviewLatestRun: ComputedRef<ReviewRunRecord | null>
  selectedReviewRuns: Ref<ReviewRunRecord[]>
  selectReview: (reviewId: string) => void
  setFriendlyError: (error: unknown) => void
  upsertLatestReviewRun: (reviewId: string, latestRun: ReviewRunRecord) => void
  upsertReviewSummary: (review: ReviewRecord, latestRun?: ReviewRunRecord | null) => void
  upsertSelectedReviewRun: (run: ReviewRunRecord) => void
}

const props = defineProps<{
  active: boolean
  context: ReviewsScreenContext
}>()

// Reviews are a separate user workflow from task dispatches even though they
// share the same shell. This screen keeps review-specific overlays, optimistic
// updates, and confirmation state together so the shell only composes the page
// rather than steering every review action itself.
const {
  cancelReviewRun,
  confirmReviewDelete,
  createReviewFromWeb,
  submitReviewFollowUp,
} = useReviewMutations({
  cancelingReviewId: props.context.cancelingReviewId,
  creatingReview: props.context.creatingReview,
  currentPage: props.context.currentPage,
  errorMessage: props.context.errorMessage,
  followingUpReview: props.context.followingUpReview,
  followingUpReviewId: props.context.followingUpReviewId,
  refreshAll: props.context.refreshAll,
  removeReview: props.context.removeReview,
  replaceSelectedReviewRuns: props.context.replaceSelectedReviewRuns,
  reviewPendingDeletion: props.context.reviewPendingDeletion,
  saving: props.context.saving,
  selectReview: props.context.selectReview,
  setFriendlyError: props.context.setFriendlyError,
  upsertLatestReviewRun: props.context.upsertLatestReviewRun,
  upsertReviewSummary: props.context.upsertReviewSummary,
  upsertSelectedReviewRun: props.context.upsertSelectedReviewRun,
})

function openNewReviewEditor() {
  props.context.creatingReview.value = true
}

function closeReviewEditor() {
  props.context.creatingReview.value = false
}

function openReviewFollowUpEditor(review = props.context.selectedReview.value) {
  if (!review) {
    return
  }

  props.context.followingUpReview.value = review
}

function closeReviewFollowUpEditor() {
  props.context.followingUpReview.value = null
}

function queueReviewDeletion(review: ReviewRecord) {
  props.context.reviewPendingDeletion.value = review
}

function clearPendingReviewDeletion() {
  props.context.reviewPendingDeletion.value = null
}

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}

function openSettingsPage() {
  props.context.currentPage.value = 'settings'
}
</script>

<template>
  <ReviewsPage
    v-if="active"
    :can-request-review="context.canRequestReview.value"
    :review-request-disabled-reason="context.reviewRequestDisabledReason.value"
    :reviews="context.reviews.value"
    @request-create-review="openNewReviewEditor"
    @request-open-settings="openSettingsPage"
    @request-select-review="context.selectReview"
  />

  <ReviewDrawer
    v-if="active && context.isReviewDrawerOpen.value && context.selectedReview.value"
    :can-cancel="context.selectedReviewCanCancel.value"
    :can-re-review="context.selectedReviewCanReReview.value"
    :canceling-review-id="context.cancelingReviewId.value"
    :following-up-review-id="context.followingUpReviewId.value"
    :latest-run="context.selectedReviewLatestRun.value"
    :review="context.selectedReview.value"
    :review-runs="context.selectedReviewRuns.value"
    :saving="context.saving.value"
    @close="context.closeReviewDrawer"
    @request-cancel-review-run="cancelReviewRun"
    @request-delete-review="queueReviewDeletion"
    @request-open-url="openExternal"
    @request-rereview="openReviewFollowUpEditor"
  />

  <ReviewRequestModal
    :busy="context.saving.value"
    :default-preferred-tool="context.defaultRemoteAgentPreferredTool.value"
    :main-user="context.remoteAgentSettings.value?.reviewFollowUp?.mainUser"
    :open="context.creatingReview.value"
    @cancel="closeReviewEditor"
    @save="createReviewFromWeb"
  />

  <ReviewFollowUpModal
    :busy="context.followingUpReviewId.value !== null"
    :open="context.followingUpReview.value !== null"
    :review="context.followingUpReview.value"
    @cancel="closeReviewFollowUpEditor"
    @save="submitReviewFollowUp"
  />

  <ConfirmDialog
    :busy="context.saving.value"
    confirm-busy-label="Deleting..."
    confirm-label="Delete review"
    confirm-variant="danger"
    :description="context.reviewPendingDeletion.value ? `Delete the saved review for ${context.reviewPendingDeletion.value.repositoryFullName} PR #${context.reviewPendingDeletion.value.pullRequestNumber}? This removes local history and remote review artifacts.` : ''"
    eyebrow="Destructive action"
    :open="context.reviewPendingDeletion.value !== null"
    title="Delete PR review"
    @cancel="clearPendingReviewDeletion"
    @confirm="confirmReviewDelete"
  />
</template>
