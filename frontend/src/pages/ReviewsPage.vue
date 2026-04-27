<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import ConfirmDialog from '../components/ConfirmDialog.vue'
import ReviewDrawer from '../components/ReviewDrawer.vue'
import ReviewFollowUpModal from '../components/ReviewFollowUpModal.vue'
import ReviewRequestModal from '../components/ReviewRequestModal.vue'
import ReviewsPageContent from '../components/ReviewsPage.vue'
import { cancelReview, createReview, deleteReview, followUpReview } from '../api/client'
import { upsertReviewRunRecord } from '../composables/useRunState'
import { replaceRouteQuery, firstQueryValue } from '../router/query'
import { useTrackerShell } from '../composables/useTrackerShell'
import type {
  ReviewRecord,
  ReviewRunRecord,
} from '../types/task'

const route = useRoute()
const router = useRouter()
const shell = useTrackerShell()

const selectedReviewRuns = ref<ReviewRunRecord[]>([])
const cancelingReviewId = ref<string | null>(null)
const followingUpReviewId = ref<string | null>(null)
const creatingReview = ref(false)
const followingUpReview = ref<ReviewRecord | null>(null)
const reviewPendingDeletion = ref<ReviewRecord | null>(null)

const selectedReviewId = computed<string | null>(() => firstQueryValue(route.query.review))
const selectedReviewSummary = computed(() =>
  shell.reviews.value.find((summary) => summary.review.id === selectedReviewId.value) ?? null,
)
const selectedReview = computed(() => selectedReviewSummary.value?.review ?? null)
const selectedReviewLatestRun = computed(() => selectedReviewSummary.value?.latestRun ?? null)
const isReviewDrawerOpen = computed(() => selectedReviewId.value !== null)
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

async function selectReview(reviewId: string) {
  await replaceRouteQuery(router, route, { review: reviewId })
}

async function closeReviewDrawer() {
  selectedReviewRuns.value = []
  followingUpReview.value = null
  await replaceRouteQuery(router, route, { review: null })
}

async function loadSelectedReviewRunHistory() {
  if (!selectedReviewId.value) {
    selectedReviewRuns.value = []
    return
  }

  try {
    selectedReviewRuns.value = await shell.loadReviewRuns(selectedReviewId.value)
  } catch (error) {
    shell.setFriendlyError(error)
  }
}

async function createReviewFromWeb(payload: Parameters<typeof createReview>[0]) {
  shell.saving.value = true
  shell.errorMessage.value = ''

  try {
    const created = await createReview(payload)
    creatingReview.value = false
    await selectReview(created.review.id)
    shell.upsertReviewSummary(created.review, created.run)
    selectedReviewRuns.value = [created.run]
    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    shell.saving.value = false
  }
}

async function confirmReviewDelete() {
  if (!reviewPendingDeletion.value) {
    return
  }

  shell.saving.value = true
  shell.errorMessage.value = ''

  try {
    const deletedReviewId = reviewPendingDeletion.value.id
    await deleteReview(deletedReviewId)
    reviewPendingDeletion.value = null
    shell.removeReview(deletedReviewId)

    if (selectedReviewId.value === deletedReviewId) {
      await closeReviewDrawer()
    }

    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    shell.saving.value = false
  }
}

function upsertSelectedReviewRun(run: ReviewRunRecord) {
  if (selectedReviewId.value !== run.reviewId) {
    return
  }

  selectedReviewRuns.value = upsertReviewRunRecord(selectedReviewRuns.value, run)
}

async function cancelReviewRunRequest(review: ReviewRecord) {
  cancelingReviewId.value = review.id
  shell.errorMessage.value = ''

  try {
    const run = await cancelReview(review.id)
    shell.upsertLatestReviewRun(review.id, run)
    upsertSelectedReviewRun(run)
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    cancelingReviewId.value = null
  }
}

async function submitReviewFollowUp(payload: Parameters<typeof followUpReview>[1]) {
  if (!followingUpReview.value) {
    return
  }

  followingUpReviewId.value = followingUpReview.value.id
  shell.errorMessage.value = ''

  try {
    const review = followingUpReview.value
    const run = await followUpReview(review.id, payload)
    shell.upsertReviewSummary(
      {
        ...review,
        updatedAt: run.createdAt,
      },
      run,
    )
    upsertSelectedReviewRun(run)
    followingUpReview.value = null
    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    followingUpReviewId.value = null
  }
}

function openNewReviewEditor() {
  creatingReview.value = true
}

function closeReviewEditor() {
  creatingReview.value = false
}

function openReviewFollowUpEditor(review = selectedReview.value) {
  if (!review) {
    return
  }

  followingUpReview.value = review
}

function closeReviewFollowUpEditor() {
  followingUpReview.value = null
}

function queueReviewDeletion(review: ReviewRecord) {
  reviewPendingDeletion.value = review
}

function clearPendingReviewDeletion() {
  reviewPendingDeletion.value = null
}

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}

function openSettingsPage() {
  void router.push({ name: 'settings' })
}

watch(selectedReviewId, () => {
  void loadSelectedReviewRunHistory()
}, { immediate: true })

watch(shell.reviews, () => {
  if (!selectedReviewId.value) {
    return
  }

  const reviewStillExists = shell.reviews.value.some((summary) => summary.review.id === selectedReviewId.value)
  if (!reviewStillExists) {
    void closeReviewDrawer()
  }
})
</script>

<template>
  <ReviewsPageContent
    :can-request-review="shell.canRequestReview.value"
    :review-request-disabled-reason="shell.reviewRequestDisabledReason.value"
    :reviews="shell.reviews.value"
    @request-create-review="openNewReviewEditor"
    @request-open-settings="openSettingsPage"
    @request-select-review="selectReview"
  />

  <ReviewDrawer
    v-if="isReviewDrawerOpen && selectedReview"
    :can-cancel="selectedReviewCanCancel"
    :can-re-review="selectedReviewCanReReview"
    :canceling-review-id="cancelingReviewId"
    :following-up-review-id="followingUpReviewId"
    :latest-run="selectedReviewLatestRun"
    :review="selectedReview"
    :review-runs="selectedReviewRuns"
    :saving="shell.saving.value"
    @close="closeReviewDrawer"
    @request-cancel-review-run="cancelReviewRunRequest"
    @request-delete-review="queueReviewDeletion"
    @request-open-url="openExternal"
    @request-rereview="openReviewFollowUpEditor"
  />

  <ReviewRequestModal
    :busy="shell.saving.value"
    :default-preferred-tool="shell.defaultRemoteAgentPreferredTool.value"
    :main-user="shell.remoteAgentSettings.value?.reviewFollowUp?.mainUser"
    :open="creatingReview"
    @cancel="closeReviewEditor"
    @save="createReviewFromWeb"
  />

  <ReviewFollowUpModal
    :busy="followingUpReviewId !== null"
    :open="followingUpReview !== null"
    :review="followingUpReview"
    @cancel="closeReviewFollowUpEditor"
    @save="submitReviewFollowUp"
  />

  <ConfirmDialog
    :busy="shell.saving.value"
    confirm-busy-label="Deleting..."
    confirm-label="Delete review"
    confirm-variant="danger"
    :description="reviewPendingDeletion ? `Delete the saved review for ${reviewPendingDeletion.repositoryFullName} PR #${reviewPendingDeletion.pullRequestNumber}? This removes local history and remote review artifacts.` : ''"
    eyebrow="Destructive action"
    :open="reviewPendingDeletion !== null"
    title="Delete PR review"
    @cancel="clearPendingReviewDeletion"
    @confirm="confirmReviewDelete"
  />
</template>
