import type { ComputedRef, Ref } from 'vue'

import type {
  ProjectInfo,
  ReviewRecord,
  Task,
} from '../types/task'
import type { PendingRunnerSetupRequest } from './useTaskMutations'

interface UseShellOverlaysOptions {
  cleanupPendingConfirmation: Ref<boolean>
  creatingReview: Ref<boolean>
  creatingTask: Ref<boolean>
  editingProject: Ref<ProjectInfo | null>
  editingRemoteAgentSetup: Ref<boolean>
  editingTask: Ref<Task | null>
  followingUpReview: Ref<ReviewRecord | null>
  followingUpTask: Ref<Task | null>
  resetPendingConfirmation: Ref<boolean>
  reviewPendingDeletion: Ref<ReviewRecord | null>
  selectedProjectDetails: ComputedRef<ProjectInfo | null>
  selectedReview: ComputedRef<ReviewRecord | null>
  taskPendingDeletion: Ref<Task | null>
  taskPendingRunnerSetup: Ref<PendingRunnerSetupRequest | null>
}

/**
 * Owns the shell's transient overlays and confirmation intent.
 *
 * These states are intentionally ephemeral. They represent "what the user is
 * trying to do right now" rather than durable application data, so grouping
 * them behind one composable keeps modal transitions explicit without
 * promoting them into a store or route layer prematurely.
 */
export function useShellOverlays(options: UseShellOverlaysOptions) {
  function openTaskEditor(task: Task) {
    options.editingTask.value = task
  }

  function openNewTaskEditor() {
    options.creatingTask.value = true
  }

  function openNewReviewEditor() {
    options.creatingReview.value = true
  }

  function openReviewFollowUpEditor(review = options.selectedReview.value) {
    if (!review) {
      return
    }

    options.followingUpReview.value = review
  }

  function openProjectEditor(project = options.selectedProjectDetails.value) {
    if (!project) {
      return
    }

    options.editingProject.value = project
  }

  function openRunnerSetup() {
    options.taskPendingRunnerSetup.value = null
    options.editingRemoteAgentSetup.value = true
  }

  function closeTaskEditor() {
    options.editingTask.value = null
    options.creatingTask.value = false
  }

  function closeReviewEditor() {
    options.creatingReview.value = false
  }

  function closeReviewFollowUpEditor() {
    options.followingUpReview.value = null
  }

  function closeProjectEditor() {
    options.editingProject.value = null
  }

  function closeRunnerSetup() {
    options.editingRemoteAgentSetup.value = false
    options.taskPendingRunnerSetup.value = null
  }

  function closeFollowUpEditor() {
    options.followingUpTask.value = null
  }

  function queueTaskDeletion(task: Task) {
    options.taskPendingDeletion.value = task
  }

  function clearPendingDeletion() {
    options.taskPendingDeletion.value = null
  }

  function queueReviewDeletion(review: ReviewRecord) {
    options.reviewPendingDeletion.value = review
  }

  function clearPendingReviewDeletion() {
    options.reviewPendingDeletion.value = null
  }

  function openRemoteCleanupConfirmation() {
    options.cleanupPendingConfirmation.value = true
  }

  function clearPendingRemoteCleanup() {
    options.cleanupPendingConfirmation.value = false
  }

  function openRemoteResetConfirmation() {
    options.resetPendingConfirmation.value = true
  }

  function clearPendingRemoteReset() {
    options.resetPendingConfirmation.value = false
  }

  return {
    clearPendingDeletion,
    clearPendingRemoteCleanup,
    clearPendingRemoteReset,
    clearPendingReviewDeletion,
    closeFollowUpEditor,
    closeProjectEditor,
    closeReviewEditor,
    closeReviewFollowUpEditor,
    closeRunnerSetup,
    closeTaskEditor,
    openNewReviewEditor,
    openNewTaskEditor,
    openProjectEditor,
    openRemoteCleanupConfirmation,
    openRemoteResetConfirmation,
    openReviewFollowUpEditor,
    openRunnerSetup,
    openTaskEditor,
    queueReviewDeletion,
    queueTaskDeletion,
  }
}
