<script setup lang="ts">
import ConfirmDialog from './ConfirmDialog.vue'
import FollowUpModal from './FollowUpModal.vue'
import TaskDrawer from './TaskDrawer.vue'
import TaskEditorModal from './TaskEditorModal.vue'
import TasksPage from './TasksPage.vue'
import { useTaskMutations } from '../composables/useTaskMutations'
import type { TasksScreenController } from '../composables/useTasksScreenController'
import { drawerPrimaryAction } from '../features/tasks/presentation'
import { taskTitle } from '../features/tasks/description'
import type { Task, TaskDispatch } from '../types/task'

const props = defineProps<{
  active: boolean
  controller: TasksScreenController
}>()

// This container is the next ownership boundary after the earlier template
// extraction. The shell still owns shared task selection and polling because
// background sync depends on them, but task-specific overlays and mutations now
// live together so App.vue no longer has to flatten that whole workflow into
// one giant prop/event surface.
const {
  confirmDelete,
  createTaskFromWeb,
  discardRunHistory,
  handlePrimaryAction,
  saveTaskEdits,
  startRemoteRun,
  submitFollowUp,
  updateTaskStatus,
} = useTaskMutations({
  cancelingDispatchTaskId: props.controller.cancelingDispatchTaskId,
  closeTaskDrawer: props.controller.closeTaskDrawer,
  creatingTask: props.controller.creatingTask,
  currentPage: props.controller.currentPage,
  discardingDispatchTaskId: props.controller.discardingDispatchTaskId,
  dispatchingTaskId: props.controller.dispatchingTaskId,
  editingTask: props.controller.editingTask,
  errorMessage: props.controller.errorMessage,
  followingUpTask: props.controller.followingUpTask,
  followingUpTaskId: props.controller.followingUpTaskId,
  isTaskDrawerOpen: props.controller.isTaskDrawerOpen,
  loadRemoteAgentSettings: props.controller.loadRemoteAgentSettings,
  loadRuns: props.controller.loadRuns,
  pendingSelectedTaskId: props.controller.pendingSelectedTaskId,
  refreshAll: props.controller.refreshAll,
  remoteAgentSettings: props.controller.remoteAgentSettings,
  removeTaskRuns: props.controller.removeTaskRuns,
  requestRunnerSetup: props.controller.requestRunnerSetup,
  runnerSetupReady: props.controller.runnerSetupReady,
  saving: props.controller.saving,
  selectedProjectFilter: props.controller.selectedProjectFilter,
  selectedTask: props.controller.selectedTask,
  selectedTaskCanContinue: props.controller.selectedTaskCanContinue,
  selectedTaskDispatchTool: props.controller.selectedTaskDispatchTool,
  selectedTaskId: props.controller.selectedTaskId,
  selectedTaskLatestDispatch: props.controller.selectedTaskLatestDispatch,
  setFriendlyError: props.controller.setFriendlyError,
  showClosed: props.controller.showClosed,
  taskLifecycleMutation: props.controller.taskLifecycleMutation,
  taskLifecycleMutationTaskId: props.controller.taskLifecycleMutationTaskId,
  taskPendingDeletion: props.controller.taskPendingDeletion,
  upsertLatestTaskDispatch: props.controller.upsertLatestTaskDispatch,
  upsertRunRecord: props.controller.upsertRunRecord,
  upsertSelectedTaskRun: props.controller.upsertSelectedTaskRun,
})

function drawerPrimaryActionLabel(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return props.controller.selectedTaskLifecycleMutation.value === 'reopening' ? 'Reopening...' : 'Reopen task'
    case 'cancel':
      return props.controller.cancelingDispatchTaskId.value === task.id ? 'Canceling...' : 'Cancel run'
    case 'continue':
      return props.controller.followingUpTaskId.value === task.id ? 'Continuing...' : 'Continue run'
    case 'start':
      return props.controller.dispatchingTaskId.value === task.id ? 'Starting...' : 'Start agent'
  }
}

function drawerPrimaryActionClass(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return 'border border-yellow/30 bg-yellow/10 text-yellow hover:bg-yellow/15'
    case 'cancel':
      return 'border border-orange/30 bg-orange/10 text-orange hover:bg-orange/15'
    case 'continue':
      return 'border border-aqua/30 bg-aqua/10 text-aqua hover:bg-aqua/15'
    case 'start':
      return 'border border-blue/30 bg-blue/10 text-blue hover:bg-blue/15'
  }
}

function openTaskEditor(task: Task) {
  props.controller.editingTask.value = task
}

function openNewTaskEditor() {
  props.controller.creatingTask.value = true
}

function closeTaskEditor() {
  props.controller.editingTask.value = null
  props.controller.creatingTask.value = false
}

function closeFollowUpEditor() {
  props.controller.followingUpTask.value = null
}

function queueTaskDeletion(task: Task) {
  props.controller.taskPendingDeletion.value = task
}

function clearPendingDeletion() {
  props.controller.taskPendingDeletion.value = null
}

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}
</script>

<template>
  <TasksPage
    v-if="active"
    :active-task-id="controller.selectedTask.value?.id ?? null"
    :drawer-open="controller.isTaskDrawerOpen.value"
    :latest-dispatch-by-task-id="controller.latestTaskDispatchesByTaskId.value"
    :projects="controller.availableProjects.value"
    :selected-project-filter="controller.selectedProjectFilter.value"
    :show-closed="controller.showClosed.value"
    :task-count="controller.tasks.value.length"
    :task-groups="controller.taskGroups.value"
    @request-create-task="openNewTaskEditor"
    @request-select-task="controller.selectTask"
    @update:selected-project-filter="controller.selectedProjectFilter.value = $event"
    @update:show-closed="controller.showClosed.value = $event"
  />

  <TaskDrawer
    v-if="active && controller.isTaskDrawerOpen.value && controller.selectedTask.value"
    :can-continue="controller.selectedTaskCanContinue.value"
    :can-discard-history="controller.selectedTaskCanDiscardHistory.value"
    :can-start-fresh="controller.selectedTaskCanStartFresh.value"
    :dispatch-disabled-reason="controller.selectedTaskDispatchDisabledReason.value"
    :is-discarding-history="controller.discardingDispatchTaskId.value === controller.selectedTask.value.id"
    :is-dispatching="controller.dispatchingTaskId.value === controller.selectedTask.value.id"
    :latest-dispatch="controller.selectedTaskLatestDispatch.value"
    :latest-reusable-pull-request="controller.selectedTaskLatestReusablePullRequest.value"
    :lifecycle-mutation="controller.selectedTaskLifecycleMutation.value"
    :lifecycle-progress-message="controller.selectedTaskLifecycleMessage.value"
    :pinned-tool="controller.selectedTaskPinnedTool.value"
    :primary-action-class="drawerPrimaryActionClass(controller.selectedTask.value, controller.selectedTaskLatestDispatch.value)"
    :primary-action-disabled="controller.selectedTaskPrimaryActionDisabled.value"
    :primary-action-label="drawerPrimaryActionLabel(controller.selectedTask.value, controller.selectedTaskLatestDispatch.value)"
    :start-tool="controller.selectedTaskDispatchTool.value"
    :task="controller.selectedTask.value"
    :task-project="controller.selectedTaskProject.value"
    :task-runs="controller.selectedTaskRuns.value"
    @close="controller.closeTaskDrawer"
    @request-close-task="updateTaskStatus(controller.selectedTask.value, 'closed')"
    @request-delete-task="queueTaskDeletion(controller.selectedTask.value)"
    @request-discard-history="discardRunHistory(controller.selectedTask.value)"
    @request-edit-task="openTaskEditor(controller.selectedTask.value)"
    @request-open-project="controller.openSelectedTaskProjectDetails"
    @request-open-url="openExternal"
    @request-primary-action="handlePrimaryAction"
    @request-start-fresh="startRemoteRun(controller.selectedTask.value)"
    @update:start-tool="controller.selectedTaskStartTool.value = $event"
  />

  <TaskEditorModal
    :busy="controller.saving.value"
    :default-project="controller.defaultCreateProject.value"
    :mode="controller.creatingTask.value ? 'create' : 'edit'"
    :open="controller.creatingTask.value || controller.editingTask.value !== null"
    :projects="controller.availableProjects.value"
    :task="controller.editingTask.value"
    @cancel="closeTaskEditor"
    @save="controller.creatingTask.value ? createTaskFromWeb($event) : saveTaskEdits($event)"
  />

  <FollowUpModal
    :busy="controller.followingUpTaskId.value !== null"
    :dispatch="controller.followingUpDispatch?.value"
    :open="controller.followingUpTask.value !== null"
    :task="controller.followingUpTask.value"
    @cancel="closeFollowUpEditor"
    @save="submitFollowUp"
  />

  <ConfirmDialog
    :busy="controller.saving.value"
    confirm-busy-label="Deleting..."
    confirm-label="Delete forever"
    confirm-variant="danger"
    :description="controller.taskPendingDeletion.value ? `Delete ${taskTitle(controller.taskPendingDeletion.value)} permanently? This cannot be undone.` : ''"
    eyebrow="Destructive action"
    :open="controller.taskPendingDeletion.value !== null"
    title="Delete task"
    @cancel="clearPendingDeletion"
    @confirm="confirmDelete"
  />
</template>
