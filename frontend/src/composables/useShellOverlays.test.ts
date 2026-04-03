import { describe, expect, it } from 'vitest'
import { computed, ref } from 'vue'

import { buildProject, buildReview, buildTask } from '../testing/factories'
import { useShellOverlays } from './useShellOverlays'

function createOverlayHarness() {
  const cleanupPendingConfirmation = ref(false)
  const creatingReview = ref(false)
  const creatingTask = ref(false)
  const editingProject = ref<ReturnType<typeof buildProject> | null>(null)
  const editingRemoteAgentSetup = ref(false)
  const editingTask = ref<ReturnType<typeof buildTask> | null>(null)
  const followingUpReviewRef = ref<ReturnType<typeof buildReview> | null>(null)
  const followingUpTask = ref<ReturnType<typeof buildTask> | null>(null)
  const resetPendingConfirmation = ref(false)
  const reviewPendingDeletion = ref<ReturnType<typeof buildReview> | null>(null)
  const selectedProjectDetailsRef = ref<ReturnType<typeof buildProject> | null>(null)
  const selectedReviewRef = ref<ReturnType<typeof buildReview> | null>(null)
  const taskPendingDeletion = ref<ReturnType<typeof buildTask> | null>(null)
  const taskPendingRunnerSetup = ref<{
    task: ReturnType<typeof buildTask>
    preferredTool: 'codex' | 'claude'
  } | null>(null)

  return {
    cleanupPendingConfirmation,
    creatingReview,
    creatingTask,
    editingProject,
    editingRemoteAgentSetup,
    editingTask,
    followingUpReviewRef,
    followingUpTask,
    resetPendingConfirmation,
    reviewPendingDeletion,
    selectedProjectDetailsRef,
    selectedReviewRef,
    taskPendingDeletion,
    taskPendingRunnerSetup,
    overlays: useShellOverlays({
      cleanupPendingConfirmation,
      creatingReview,
      creatingTask,
      editingProject,
      editingRemoteAgentSetup,
      editingTask,
      followingUpReview: followingUpReviewRef,
      followingUpTask,
      resetPendingConfirmation,
      reviewPendingDeletion,
      selectedProjectDetails: computed(() => selectedProjectDetailsRef.value),
      selectedReview: computed(() => selectedReviewRef.value),
      taskPendingDeletion,
      taskPendingRunnerSetup,
    }),
  }
}

describe('useShellOverlays', () => {
  it('opens and closes editor overlays from the current shell selection', () => {
    const harness = createOverlayHarness()
    const task = buildTask()
    const review = buildReview()
    const project = buildProject()

    harness.selectedReviewRef.value = review
    harness.selectedProjectDetailsRef.value = project

    harness.overlays.openNewTaskEditor()
    harness.overlays.openTaskEditor(task)
    harness.overlays.openNewReviewEditor()
    harness.overlays.openReviewFollowUpEditor()
    harness.overlays.openProjectEditor()

    expect(harness.creatingTask.value).toBe(true)
    expect(harness.editingTask.value).toEqual(task)
    expect(harness.creatingReview.value).toBe(true)
    expect(harness.followingUpReviewRef.value).toEqual(review)
    expect(harness.editingProject.value).toEqual(project)

    harness.taskPendingRunnerSetup.value = { task, preferredTool: 'claude' }
    harness.overlays.openRunnerSetup()
    expect(harness.editingRemoteAgentSetup.value).toBe(true)
    expect(harness.taskPendingRunnerSetup.value).toBeNull()

    harness.overlays.closeTaskEditor()
    harness.overlays.closeReviewEditor()
    harness.overlays.closeReviewFollowUpEditor()
    harness.overlays.closeProjectEditor()
    harness.overlays.closeRunnerSetup()

    expect(harness.creatingTask.value).toBe(false)
    expect(harness.editingTask.value).toBeNull()
    expect(harness.creatingReview.value).toBe(false)
    expect(harness.followingUpReviewRef.value).toBeNull()
    expect(harness.editingProject.value).toBeNull()
    expect(harness.editingRemoteAgentSetup.value).toBe(false)
  })

  it('tracks destructive confirmations and follow-up modal intent', () => {
    const harness = createOverlayHarness()
    const task = buildTask()
    const review = buildReview()

    harness.overlays.queueTaskDeletion(task)
    harness.overlays.queueReviewDeletion(review)
    harness.overlays.openRemoteCleanupConfirmation()
    harness.overlays.openRemoteResetConfirmation()
    harness.followingUpTask.value = task

    expect(harness.taskPendingDeletion.value).toEqual(task)
    expect(harness.reviewPendingDeletion.value).toEqual(review)
    expect(harness.cleanupPendingConfirmation.value).toBe(true)
    expect(harness.resetPendingConfirmation.value).toBe(true)

    harness.overlays.closeFollowUpEditor()
    harness.overlays.clearPendingDeletion()
    harness.overlays.clearPendingReviewDeletion()
    harness.overlays.clearPendingRemoteCleanup()
    harness.overlays.clearPendingRemoteReset()

    expect(harness.followingUpTask.value).toBeNull()
    expect(harness.taskPendingDeletion.value).toBeNull()
    expect(harness.reviewPendingDeletion.value).toBeNull()
    expect(harness.cleanupPendingConfirmation.value).toBe(false)
    expect(harness.resetPendingConfirmation.value).toBe(false)
  })
})
