import { computed, ref, type ComputedRef, type Ref } from 'vue'

import { groupTasksByProject, type TaskGroup } from '../features/tasks/presentation'
import type {
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'
import { useTaskViewState } from './useTaskViewState'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'
type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

interface UseTasksScreenControllerOptions {
  data: {
    availableProjects: ComputedRef<ProjectInfo[]>
    defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
    remoteAgentSettings: Ref<RemoteAgentSettings | null>
    runnerSetupReady: ComputedRef<boolean>
    saving: Ref<boolean>
    tasks: Ref<Task[]>
  }
  project: {
    selectProjectDetails: (project: ProjectInfo) => void
  }
  settings: {
    requestRunnerSetup: (task: Task, preferredTool: RemoteAgentPreferredTool) => void
  }
  shell: {
    currentPage: Ref<AppPage>
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
}

/**
 * Owns the task screen's local state while keeping the shell-level bridges explicit.
 *
 * The task queue now has enough independent behavior that its drawer, filters,
 * and mutation workflow refs are easier to reason about as one domain object.
 * The shell still supplies shared data and persistence bridges, but the task
 * screen now owns its own UI intent instead of borrowing a long list of refs
 * from App.vue.
 */
export function useTasksScreenController(options: UseTasksScreenControllerOptions) {
  const latestTaskDispatchesByTaskId = ref<Record<string, TaskDispatch>>({})
  const selectedTaskRuns = ref<RunRecord[]>([])

  const cancelingDispatchTaskId = ref<string | null>(null)
  const discardingDispatchTaskId = ref<string | null>(null)
  const dispatchingTaskId = ref<string | null>(null)
  const followingUpTaskId = ref<string | null>(null)
  const taskLifecycleMutation = ref<TaskLifecycleMutation | null>(null)
  const taskLifecycleMutationTaskId = ref<string | null>(null)

  const creatingTask = ref(false)
  const editingTask = ref<Task | null>(null)
  const followingUpTask = ref<Task | null>(null)
  const taskPendingDeletion = ref<Task | null>(null)
  const latestDispatchByTaskId = computed(() => latestTaskDispatchesByTaskId.value)

  const viewState = useTaskViewState({
    availableProjects: options.data.availableProjects,
    cancelingDispatchTaskId,
    currentPage: options.shell.currentPage,
    defaultRemoteAgentPreferredTool: options.data.defaultRemoteAgentPreferredTool,
    dispatchingTaskId,
    followingUpTaskId,
    latestDispatchByTaskId,
    remoteAgentSettings: options.data.remoteAgentSettings,
    selectedTaskRuns,
    taskLifecycleMutation,
    taskLifecycleMutationTaskId,
    tasks: options.data.tasks,
  })

  const taskGroups = computed<TaskGroup[]>(() => groupTasksByProject(options.data.tasks.value))
  const defaultCreateProject = computed(
    () =>
      viewState.selectedProjectFilter.value ||
      options.data.availableProjects.value[0]?.canonicalName ||
      '',
  )
  const followingUpDispatch = computed(() =>
    followingUpTask.value
      ? latestDispatchByTaskId.value[followingUpTask.value.id] ?? undefined
      : undefined,
  )

  function openSelectedTaskProjectDetails() {
    if (!viewState.selectedTaskProject.value) {
      return
    }

    options.project.selectProjectDetails(viewState.selectedTaskProject.value)
  }

  return {
    availableProjects: options.data.availableProjects,
    cancelingDispatchTaskId,
    closeTaskDrawer: viewState.closeTaskDrawer,
    creatingTask,
    currentPage: options.shell.currentPage,
    defaultCreateProject,
    dispatchingTaskId,
    discardingDispatchTaskId,
    editingTask,
    errorMessage: options.shell.errorMessage,
    followingUpDispatch,
    followingUpTask,
    followingUpTaskId,
    isTaskDrawerOpen: viewState.isTaskDrawerOpen,
    latestTaskDispatchesByTaskId,
    loadRemoteAgentSettings: options.taskRunBridge.loadRemoteAgentSettings,
    loadRuns: options.taskRunBridge.loadRuns,
    openSelectedTaskProjectDetails,
    openTaskFromRun: viewState.openTaskFromRun,
    pendingSelectedTaskId: viewState.pendingSelectedTaskId,
    refreshAll: options.taskRunBridge.refreshAll,
    remoteAgentSettings: options.data.remoteAgentSettings,
    removeTaskRuns: options.taskRunBridge.removeTaskRuns,
    requestRunnerSetup: options.settings.requestRunnerSetup,
    runnerSetupReady: options.data.runnerSetupReady,
    saving: options.data.saving,
    selectedProjectFilter: viewState.selectedProjectFilter,
    selectedTask: viewState.selectedTask,
    selectedTaskCanContinue: viewState.selectedTaskCanContinue,
    selectedTaskCanDiscardHistory: viewState.selectedTaskCanDiscardHistory,
    selectedTaskCanStartFresh: viewState.selectedTaskCanStartFresh,
    selectedTaskDispatchDisabledReason: viewState.selectedTaskDispatchDisabledReason,
    selectedTaskDispatchTool: viewState.selectedTaskDispatchTool,
    selectedTaskId: viewState.selectedTaskId,
    selectedTaskLatestDispatch: viewState.selectedTaskLatestDispatch,
    selectedTaskLatestReusablePullRequest: viewState.selectedTaskLatestReusablePullRequest,
    selectedTaskLifecycleMessage: viewState.selectedTaskLifecycleMessage,
    selectedTaskLifecycleMutation: viewState.selectedTaskLifecycleMutation,
    selectedTaskPinnedTool: viewState.selectedTaskPinnedTool,
    selectedTaskPrimaryActionDisabled: viewState.selectedTaskPrimaryActionDisabled,
    selectedTaskProject: viewState.selectedTaskProject,
    selectedTaskRuns,
    selectedTaskStartTool: viewState.selectedTaskStartTool,
    selectTask: viewState.selectTask,
    setFriendlyError: options.shell.setFriendlyError,
    showClosed: viewState.showClosed,
    taskGroups,
    taskLifecycleMutation,
    taskLifecycleMutationTaskId,
    taskPendingDeletion,
    tasks: options.data.tasks,
    upsertLatestTaskDispatch: options.taskRunBridge.upsertLatestTaskDispatch,
    upsertRunRecord: options.taskRunBridge.upsertRunRecord,
    upsertSelectedTaskRun: options.taskRunBridge.upsertSelectedTaskRun,
  }
}

export type TasksScreenController = ReturnType<typeof useTasksScreenController>
