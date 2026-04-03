<script setup lang="ts">
import ConfirmDialog from './ConfirmDialog.vue'
import ReviewDrawer from './ReviewDrawer.vue'
import ReviewFollowUpModal from './ReviewFollowUpModal.vue'
import ReviewRequestModal from './ReviewRequestModal.vue'
import ReviewsPage from './ReviewsPage.vue'
import { useReviewMutations } from '../composables/useReviewMutations'
import type { ReviewsScreenController } from '../composables/useReviewsScreenController'
import type {
  ReviewRecord,
} from '../types/task'

const props = defineProps<{
  active: boolean
  controller: ReviewsScreenController
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
  cancelingReviewId: props.controller.cancelingReviewId,
  creatingReview: props.controller.creatingReview,
  currentPage: props.controller.currentPage,
  errorMessage: props.controller.errorMessage,
  followingUpReview: props.controller.followingUpReview,
  followingUpReviewId: props.controller.followingUpReviewId,
  refreshAll: props.controller.refreshAll,
  removeReview: props.controller.removeReview,
  replaceSelectedReviewRuns: props.controller.replaceSelectedReviewRuns,
  reviewPendingDeletion: props.controller.reviewPendingDeletion,
  saving: props.controller.saving,
  selectReview: props.controller.selectReview,
  setFriendlyError: props.controller.setFriendlyError,
  upsertLatestReviewRun: props.controller.upsertLatestReviewRun,
  upsertReviewSummary: props.controller.upsertReviewSummary,
  upsertSelectedReviewRun: props.controller.upsertSelectedReviewRun,
})

function openNewReviewEditor() {
  props.controller.creatingReview.value = true
}

function closeReviewEditor() {
  props.controller.creatingReview.value = false
}

function openReviewFollowUpEditor(review = props.controller.selectedReview.value) {
  if (!review) {
    return
  }

  props.controller.followingUpReview.value = review
}

function closeReviewFollowUpEditor() {
  props.controller.followingUpReview.value = null
}

function queueReviewDeletion(review: ReviewRecord) {
  props.controller.reviewPendingDeletion.value = review
}

function clearPendingReviewDeletion() {
  props.controller.reviewPendingDeletion.value = null
}

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}

function openSettingsPage() {
  props.controller.currentPage.value = 'settings'
}
</script>

<template>
  <ReviewsPage
    v-if="active"
    :can-request-review="controller.canRequestReview.value"
    :review-request-disabled-reason="controller.reviewRequestDisabledReason.value"
    :reviews="controller.reviews.value"
    @request-create-review="openNewReviewEditor"
    @request-open-settings="openSettingsPage"
    @request-select-review="controller.selectReview"
  />

  <ReviewDrawer
    v-if="active && controller.isReviewDrawerOpen.value && controller.selectedReview.value"
    :can-cancel="controller.selectedReviewCanCancel.value"
    :can-re-review="controller.selectedReviewCanReReview.value"
    :canceling-review-id="controller.cancelingReviewId.value"
    :following-up-review-id="controller.followingUpReviewId.value"
    :latest-run="controller.selectedReviewLatestRun.value"
    :review="controller.selectedReview.value"
    :review-runs="controller.selectedReviewRuns.value"
    :saving="controller.saving.value"
    @close="controller.closeReviewDrawer"
    @request-cancel-review-run="cancelReviewRun"
    @request-delete-review="queueReviewDeletion"
    @request-open-url="openExternal"
    @request-rereview="openReviewFollowUpEditor"
  />

  <ReviewRequestModal
    :busy="controller.saving.value"
    :default-preferred-tool="controller.defaultRemoteAgentPreferredTool.value"
    :main-user="controller.remoteAgentSettings.value?.reviewFollowUp?.mainUser"
    :open="controller.creatingReview.value"
    @cancel="closeReviewEditor"
    @save="createReviewFromWeb"
  />

  <ReviewFollowUpModal
    :busy="controller.followingUpReviewId.value !== null"
    :open="controller.followingUpReview.value !== null"
    :review="controller.followingUpReview.value"
    @cancel="closeReviewFollowUpEditor"
    @save="submitReviewFollowUp"
  />

  <ConfirmDialog
    :busy="controller.saving.value"
    confirm-busy-label="Deleting..."
    confirm-label="Delete review"
    confirm-variant="danger"
    :description="controller.reviewPendingDeletion.value ? `Delete the saved review for ${controller.reviewPendingDeletion.value.repositoryFullName} PR #${controller.reviewPendingDeletion.value.pullRequestNumber}? This removes local history and remote review artifacts.` : ''"
    eyebrow="Destructive action"
    :open="controller.reviewPendingDeletion.value !== null"
    title="Delete PR review"
    @cancel="clearPendingReviewDeletion"
    @confirm="confirmReviewDelete"
  />
</template>
