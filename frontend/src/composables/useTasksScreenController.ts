import type { ComputedRef, Ref } from 'vue'

import type { TaskGroup } from '../features/tasks/presentation'
import type {
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'
import type { PendingRunnerSetupRequest } from './useTaskMutations'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'
type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

interface TaskViewState {
  closeTaskDrawer: () => void
  isTaskDrawerOpen: Ref<boolean>
  pendingSelectedTaskId: Ref<string | null>
  selectTask: (taskId: string) => void
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
  selectedTaskStartTool: Ref<RemoteAgentPreferredTool>
  showClosed: Ref<boolean>
}

interface UseTasksScreenControllerOptions {
  data: {
    availableProjects: ComputedRef<ProjectInfo[]>
    defaultCreateProject: ComputedRef<string>
    followingUpDispatch: ComputedRef<TaskDispatch | undefined>
    latestTaskDispatchesByTaskId: Ref<Record<string, TaskDispatch>>
    remoteAgentSettings: Ref<RemoteAgentSettings | null>
    runnerSetupReady: ComputedRef<boolean>
    saving: Ref<boolean>
    selectedTaskRuns: Ref<RunRecord[]>
    taskGroups: ComputedRef<TaskGroup[]>
    tasks: Ref<Task[]>
  }
  overlays: {
    creatingTask: Ref<boolean>
    editingTask: Ref<Task | null>
    followingUpTask: Ref<Task | null>
    taskPendingDeletion: Ref<Task | null>
    taskPendingRunnerSetup: Ref<PendingRunnerSetupRequest | null>
  }
  project: {
    openSelectedTaskProjectDetails: () => void
  }
  shell: {
    currentPage: Ref<AppPage>
    editingRemoteAgentSetup: Ref<boolean>
    errorMessage: Ref<string>
    setFriendlyError: (error: unknown) => void
  }
  taskRunBridge: {
    loadRemoteAgentSettings: () => Promise<void>
    loadRuns: () => Promise<void>
    refreshAll: () => Promise<void>
    removeTaskRuns: (taskId: string) => void
    upsertLatestTaskDispatch: (dispatch: TaskDispatch) => void
    upsertRunRecord: (task: Task, dispatch: TaskDispatch) => void
    upsertSelectedTaskRun: (task: Task, dispatch: TaskDispatch) => void
  }
  viewState: TaskViewState
  workflow: {
    cancelingDispatchTaskId: Ref<string | null>
    discardingDispatchTaskId: Ref<string | null>
    dispatchingTaskId: Ref<string | null>
    followingUpTaskId: Ref<string | null>
    taskLifecycleMutation: Ref<TaskLifecycleMutation | null>
    taskLifecycleMutationTaskId: Ref<string | null>
  }
}

/**
 * Shapes all task-screen dependencies into one named boundary.
 *
 * App.vue still owns the underlying refs today, but it no longer has to
 * flatten every task concern into an ad-hoc object literal beside the template.
 * This composable marks the task screen's dependency surface explicitly so the
 * next controller phase can move ownership inward without changing callers.
 */
export function useTasksScreenController(options: UseTasksScreenControllerOptions) {
  return {
    availableProjects: options.data.availableProjects,
    cancelingDispatchTaskId: options.workflow.cancelingDispatchTaskId,
    closeTaskDrawer: options.viewState.closeTaskDrawer,
    creatingTask: options.overlays.creatingTask,
    currentPage: options.shell.currentPage,
    defaultCreateProject: options.data.defaultCreateProject,
    dispatchingTaskId: options.workflow.dispatchingTaskId,
    discardingDispatchTaskId: options.workflow.discardingDispatchTaskId,
    editingRemoteAgentSetup: options.shell.editingRemoteAgentSetup,
    editingTask: options.overlays.editingTask,
    errorMessage: options.shell.errorMessage,
    followingUpDispatch: options.data.followingUpDispatch,
    followingUpTask: options.overlays.followingUpTask,
    followingUpTaskId: options.workflow.followingUpTaskId,
    isTaskDrawerOpen: options.viewState.isTaskDrawerOpen,
    latestTaskDispatchesByTaskId: options.data.latestTaskDispatchesByTaskId,
    loadRemoteAgentSettings: options.taskRunBridge.loadRemoteAgentSettings,
    loadRuns: options.taskRunBridge.loadRuns,
    openSelectedTaskProjectDetails: options.project.openSelectedTaskProjectDetails,
    pendingSelectedTaskId: options.viewState.pendingSelectedTaskId,
    refreshAll: options.taskRunBridge.refreshAll,
    remoteAgentSettings: options.data.remoteAgentSettings,
    removeTaskRuns: options.taskRunBridge.removeTaskRuns,
    runnerSetupReady: options.data.runnerSetupReady,
    saving: options.data.saving,
    selectedProjectFilter: options.viewState.selectedProjectFilter,
    selectedTask: options.viewState.selectedTask,
    selectedTaskCanContinue: options.viewState.selectedTaskCanContinue,
    selectedTaskCanDiscardHistory: options.viewState.selectedTaskCanDiscardHistory,
    selectedTaskCanStartFresh: options.viewState.selectedTaskCanStartFresh,
    selectedTaskDispatchDisabledReason: options.viewState.selectedTaskDispatchDisabledReason,
    selectedTaskDispatchTool: options.viewState.selectedTaskDispatchTool,
    selectedTaskId: options.viewState.selectedTaskId,
    selectedTaskLatestDispatch: options.viewState.selectedTaskLatestDispatch,
    selectedTaskLatestReusablePullRequest: options.viewState.selectedTaskLatestReusablePullRequest,
    selectedTaskLifecycleMessage: options.viewState.selectedTaskLifecycleMessage,
    selectedTaskLifecycleMutation: options.viewState.selectedTaskLifecycleMutation,
    selectedTaskPinnedTool: options.viewState.selectedTaskPinnedTool,
    selectedTaskPrimaryActionDisabled: options.viewState.selectedTaskPrimaryActionDisabled,
    selectedTaskProject: options.viewState.selectedTaskProject,
    selectedTaskRuns: options.data.selectedTaskRuns,
    selectedTaskStartTool: options.viewState.selectedTaskStartTool,
    selectTask: options.viewState.selectTask,
    setFriendlyError: options.shell.setFriendlyError,
    showClosed: options.viewState.showClosed,
    taskGroups: options.data.taskGroups,
    taskLifecycleMutation: options.workflow.taskLifecycleMutation,
    taskLifecycleMutationTaskId: options.workflow.taskLifecycleMutationTaskId,
    taskPendingDeletion: options.overlays.taskPendingDeletion,
    taskPendingRunnerSetup: options.overlays.taskPendingRunnerSetup,
    tasks: options.data.tasks,
    upsertLatestTaskDispatch: options.taskRunBridge.upsertLatestTaskDispatch,
    upsertRunRecord: options.taskRunBridge.upsertRunRecord,
    upsertSelectedTaskRun: options.taskRunBridge.upsertSelectedTaskRun,
  }
}

export type TasksScreenController = ReturnType<typeof useTasksScreenController>
