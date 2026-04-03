<script setup lang="ts">
import type { ComputedRef, Ref } from 'vue'

import ConfirmDialog from './ConfirmDialog.vue'
import FollowUpModal from './FollowUpModal.vue'
import TaskDrawer from './TaskDrawer.vue'
import TaskEditorModal from './TaskEditorModal.vue'
import TasksPage from './TasksPage.vue'
import { useTaskMutations, type PendingRunnerSetupRequest } from '../composables/useTaskMutations'
import { drawerPrimaryAction, type TaskGroup } from '../features/tasks/presentation'
import { taskTitle } from '../features/tasks/description'
import type {
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'
type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

interface TasksScreenContext {
  availableProjects: ComputedRef<ProjectInfo[]>
  cancelingDispatchTaskId: Ref<string | null>
  closeTaskDrawer: () => void
  creatingTask: Ref<boolean>
  currentPage: Ref<AppPage>
  defaultCreateProject: ComputedRef<string>
  dispatchingTaskId: Ref<string | null>
  discardingDispatchTaskId: Ref<string | null>
  editingRemoteAgentSetup: Ref<boolean>
  editingTask: Ref<Task | null>
  followingUpTask: Ref<Task | null>
  followingUpTaskId: Ref<string | null>
  isTaskDrawerOpen: Ref<boolean>
  latestTaskDispatchesByTaskId: Ref<Record<string, TaskDispatch>>
  loadRemoteAgentSettings: () => Promise<void>
  loadRuns: () => Promise<void>
  openSelectedTaskProjectDetails: () => void
  pendingSelectedTaskId: Ref<string | null>
  refreshAll: () => Promise<void>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  removeTaskRuns: (taskId: string) => void
  runnerSetupReady: ComputedRef<boolean>
  saving: Ref<boolean>
  selectedProjectFilter: Ref<string>
  selectedTask: ComputedRef<Task | null>
  selectedTaskCanContinue: ComputedRef<boolean>
  selectedTaskCanDiscardHistory: ComputedRef<boolean>
  selectedTaskCanStartFresh: ComputedRef<boolean>
  selectedTaskDispatchDisabledReason: ComputedRef<string | undefined>
  selectedTaskDispatchTool: ComputedRef<RemoteAgentPreferredTool>
  selectedTaskId: Ref<string | null>
  selectedTaskLatestDispatch: ComputedRef<TaskDispatch | null>
  selectedTaskLatestReusablePullRequest: ComputedRef<string | null>
  selectedTaskLifecycleMessage: ComputedRef<string>
  selectedTaskLifecycleMutation: ComputedRef<TaskLifecycleMutation | null>
  selectedTaskPinnedTool: ComputedRef<RemoteAgentPreferredTool | null>
  selectedTaskPrimaryActionDisabled: ComputedRef<boolean>
  selectedTaskProject: ComputedRef<ProjectInfo | null>
  selectedTaskRuns: Ref<RunRecord[]>
  selectedTaskStartTool: Ref<RemoteAgentPreferredTool>
  selectTask: (taskId: string) => void
  setFriendlyError: (error: unknown) => void
  showClosed: Ref<boolean>
  taskGroups: ComputedRef<TaskGroup[]>
  taskLifecycleMutation: Ref<TaskLifecycleMutation | null>
  taskLifecycleMutationTaskId: Ref<string | null>
  taskPendingDeletion: Ref<Task | null>
  taskPendingRunnerSetup: Ref<PendingRunnerSetupRequest | null>
  tasks: Ref<Task[]>
  upsertLatestTaskDispatch: (dispatch: TaskDispatch) => void
  upsertRunRecord: (task: Task, dispatch: TaskDispatch) => void
  upsertSelectedTaskRun: (task: Task, dispatch: TaskDispatch) => void
  errorMessage: Ref<string>
  followingUpDispatch: ComputedRef<TaskDispatch | undefined>
}

const props = defineProps<{
  active: boolean
  context: TasksScreenContext
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
  cancelingDispatchTaskId: props.context.cancelingDispatchTaskId,
  closeTaskDrawer: props.context.closeTaskDrawer,
  creatingTask: props.context.creatingTask,
  currentPage: props.context.currentPage,
  discardingDispatchTaskId: props.context.discardingDispatchTaskId,
  dispatchingTaskId: props.context.dispatchingTaskId,
  editingRemoteAgentSetup: props.context.editingRemoteAgentSetup,
  editingTask: props.context.editingTask,
  errorMessage: props.context.errorMessage,
  followingUpTask: props.context.followingUpTask,
  followingUpTaskId: props.context.followingUpTaskId,
  isTaskDrawerOpen: props.context.isTaskDrawerOpen,
  loadRemoteAgentSettings: props.context.loadRemoteAgentSettings,
  loadRuns: props.context.loadRuns,
  pendingSelectedTaskId: props.context.pendingSelectedTaskId,
  refreshAll: props.context.refreshAll,
  remoteAgentSettings: props.context.remoteAgentSettings,
  removeTaskRuns: props.context.removeTaskRuns,
  runnerSetupReady: props.context.runnerSetupReady,
  saving: props.context.saving,
  selectedProjectFilter: props.context.selectedProjectFilter,
  selectedTask: props.context.selectedTask,
  selectedTaskCanContinue: props.context.selectedTaskCanContinue,
  selectedTaskDispatchTool: props.context.selectedTaskDispatchTool,
  selectedTaskId: props.context.selectedTaskId,
  selectedTaskLatestDispatch: props.context.selectedTaskLatestDispatch,
  setFriendlyError: props.context.setFriendlyError,
  showClosed: props.context.showClosed,
  taskLifecycleMutation: props.context.taskLifecycleMutation,
  taskLifecycleMutationTaskId: props.context.taskLifecycleMutationTaskId,
  taskPendingDeletion: props.context.taskPendingDeletion,
  taskPendingRunnerSetup: props.context.taskPendingRunnerSetup,
  upsertLatestTaskDispatch: props.context.upsertLatestTaskDispatch,
  upsertRunRecord: props.context.upsertRunRecord,
  upsertSelectedTaskRun: props.context.upsertSelectedTaskRun,
})

function drawerPrimaryActionLabel(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return props.context.selectedTaskLifecycleMutation.value === 'reopening' ? 'Reopening...' : 'Reopen task'
    case 'cancel':
      return props.context.cancelingDispatchTaskId.value === task.id ? 'Canceling...' : 'Cancel run'
    case 'continue':
      return props.context.followingUpTaskId.value === task.id ? 'Continuing...' : 'Continue run'
    case 'start':
      return props.context.dispatchingTaskId.value === task.id ? 'Starting...' : 'Start agent'
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
  props.context.editingTask.value = task
}

function openNewTaskEditor() {
  props.context.creatingTask.value = true
}

function closeTaskEditor() {
  props.context.editingTask.value = null
  props.context.creatingTask.value = false
}

function closeFollowUpEditor() {
  props.context.followingUpTask.value = null
}

function queueTaskDeletion(task: Task) {
  props.context.taskPendingDeletion.value = task
}

function clearPendingDeletion() {
  props.context.taskPendingDeletion.value = null
}

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}
</script>

<template>
  <TasksPage
    v-if="active"
    :active-task-id="context.selectedTask.value?.id ?? null"
    :drawer-open="context.isTaskDrawerOpen.value"
    :latest-dispatch-by-task-id="context.latestTaskDispatchesByTaskId.value"
    :projects="context.availableProjects.value"
    :selected-project-filter="context.selectedProjectFilter.value"
    :show-closed="context.showClosed.value"
    :task-count="context.tasks.value.length"
    :task-groups="context.taskGroups.value"
    @request-create-task="openNewTaskEditor"
    @request-select-task="context.selectTask"
    @update:selected-project-filter="context.selectedProjectFilter.value = $event"
    @update:show-closed="context.showClosed.value = $event"
  />

  <TaskDrawer
    v-if="active && context.isTaskDrawerOpen.value && context.selectedTask.value"
    :can-continue="context.selectedTaskCanContinue.value"
    :can-discard-history="context.selectedTaskCanDiscardHistory.value"
    :can-start-fresh="context.selectedTaskCanStartFresh.value"
    :dispatch-disabled-reason="context.selectedTaskDispatchDisabledReason.value"
    :is-discarding-history="context.discardingDispatchTaskId.value === context.selectedTask.value.id"
    :is-dispatching="context.dispatchingTaskId.value === context.selectedTask.value.id"
    :latest-dispatch="context.selectedTaskLatestDispatch.value"
    :latest-reusable-pull-request="context.selectedTaskLatestReusablePullRequest.value"
    :lifecycle-mutation="context.selectedTaskLifecycleMutation.value"
    :lifecycle-progress-message="context.selectedTaskLifecycleMessage.value"
    :pinned-tool="context.selectedTaskPinnedTool.value"
    :primary-action-class="drawerPrimaryActionClass(context.selectedTask.value, context.selectedTaskLatestDispatch.value)"
    :primary-action-disabled="context.selectedTaskPrimaryActionDisabled.value"
    :primary-action-label="drawerPrimaryActionLabel(context.selectedTask.value, context.selectedTaskLatestDispatch.value)"
    :start-tool="context.selectedTaskDispatchTool.value"
    :task="context.selectedTask.value"
    :task-project="context.selectedTaskProject.value"
    :task-runs="context.selectedTaskRuns.value"
    @close="context.closeTaskDrawer"
    @request-close-task="updateTaskStatus(context.selectedTask.value, 'closed')"
    @request-delete-task="queueTaskDeletion(context.selectedTask.value)"
    @request-discard-history="discardRunHistory(context.selectedTask.value)"
    @request-edit-task="openTaskEditor(context.selectedTask.value)"
    @request-open-project="context.openSelectedTaskProjectDetails"
    @request-open-url="openExternal"
    @request-primary-action="handlePrimaryAction"
    @request-start-fresh="startRemoteRun(context.selectedTask.value)"
    @update:start-tool="context.selectedTaskStartTool.value = $event"
  />

  <TaskEditorModal
    :busy="context.saving.value"
    :default-project="context.defaultCreateProject.value"
    :mode="context.creatingTask.value ? 'create' : 'edit'"
    :open="context.creatingTask.value || context.editingTask.value !== null"
    :projects="context.availableProjects.value"
    :task="context.editingTask.value"
    @cancel="closeTaskEditor"
    @save="context.creatingTask.value ? createTaskFromWeb($event) : saveTaskEdits($event)"
  />

  <FollowUpModal
    :busy="context.followingUpTaskId.value !== null"
    :dispatch="context.followingUpDispatch?.value"
    :open="context.followingUpTask.value !== null"
    :task="context.followingUpTask.value"
    @cancel="closeFollowUpEditor"
    @save="submitFollowUp"
  />

  <ConfirmDialog
    :busy="context.saving.value"
    confirm-busy-label="Deleting..."
    confirm-label="Delete forever"
    confirm-variant="danger"
    :description="context.taskPendingDeletion.value ? `Delete ${taskTitle(context.taskPendingDeletion.value)} permanently? This cannot be undone.` : ''"
    eyebrow="Destructive action"
    :open="context.taskPendingDeletion.value !== null"
    title="Delete task"
    @cancel="clearPendingDeletion"
    @confirm="confirmDelete"
  />
</template>
