import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'

import { getRunStartDisabledReason } from '../features/tasks/presentation'
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

interface UseTaskViewStateOptions {
  availableProjects: ComputedRef<ProjectInfo[]>
  cancelingDispatchTaskId: Ref<string | null>
  currentPage: Ref<AppPage>
  defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
  dispatchingTaskId: Ref<string | null>
  followingUpTaskId: Ref<string | null>
  latestDispatchByTaskId: ComputedRef<Record<string, TaskDispatch>>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  selectedTaskRuns: Ref<RunRecord[]>
  taskLifecycleMutation: Ref<TaskLifecycleMutation | null>
  taskLifecycleMutationTaskId: Ref<string | null>
  tasks: Ref<Task[]>
}

function taskLifecycleProgressMessage(mutation: TaskLifecycleMutation | null): string {
  switch (mutation) {
    case 'closing':
      return 'Closing the task and cleaning up its remote worktree...'
    case 'reopening':
      return 'Reopening the task so you can continue work...'
    case 'deleting':
      return 'Deleting the task and removing its remote artifacts...'
    case null:
      return ''
  }
}

/**
 * Coordinates the task queue, task drawer, and task-scoped run history.
 *
 * The task surface has a few easy-to-miss reactivity edge cases:
 * filters can temporarily hide the selected task, and opening a run from the
 * Runs page may need to widen filters before the row exists again.
 * Centralizing that behavior here keeps App.vue focused on mutations and API
 * orchestration instead of queue-specific selection edge cases.
 */
export function useTaskViewState(options: UseTaskViewStateOptions) {
  const showClosed = ref(false)
  const selectedProjectFilter = ref('')
  const selectedTaskId = ref<string | null>(null)
  const pendingSelectedTaskId = ref<string | null>(null)
  const isTaskDrawerOpen = ref(false)
  const selectedTaskStartTool = ref<RemoteAgentPreferredTool>('codex')

  const selectedTask = computed(() =>
    options.tasks.value.find((task) => task.id === selectedTaskId.value) ?? null,
  )

  const selectedTaskProject = computed(() =>
    selectedTask.value
      ? options.availableProjects.value.find((project) => project.canonicalName === selectedTask.value?.project) ?? null
      : null,
  )

  const selectedTaskLatestDispatch = computed(() =>
    selectedTask.value ? options.latestDispatchByTaskId.value[selectedTask.value.id] ?? null : null,
  )

  const selectedTaskPinnedTool = computed<RemoteAgentPreferredTool | null>(
    () => selectedTaskLatestDispatch.value?.preferredTool ?? null,
  )

  const selectedTaskDispatchTool = computed<RemoteAgentPreferredTool>(
    () => selectedTaskPinnedTool.value ?? selectedTaskStartTool.value,
  )

  const selectedTaskLatestReusablePullRequest = computed(() =>
    options.selectedTaskRuns.value.find((run) => Boolean(run.dispatch.pullRequestUrl))?.dispatch.pullRequestUrl
      ?? selectedTaskLatestDispatch.value?.pullRequestUrl
      ?? null,
  )

  const selectedTaskLifecycleMutation = computed(() =>
    selectedTask.value && options.taskLifecycleMutationTaskId.value === selectedTask.value.id
      ? options.taskLifecycleMutation.value
      : null,
  )

  const selectedTaskDispatchDisabledReason = computed(() =>
    selectedTask.value
      ? getRunStartDisabledReason(
        selectedTask.value,
        options.availableProjects.value,
        options.remoteAgentSettings.value,
      )
      : undefined,
  )

  const selectedTaskCanContinue = computed(() =>
    Boolean(
      selectedTask.value &&
        selectedTaskLatestDispatch.value &&
        !selectedTaskDispatchDisabledReason.value &&
        selectedTaskLatestDispatch.value.status !== 'preparing' &&
        selectedTaskLatestDispatch.value.status !== 'running' &&
        selectedTaskLatestDispatch.value.branchName &&
        selectedTaskLatestDispatch.value.worktreePath,
    ),
  )

  const selectedTaskCanStartFresh = computed(() =>
    Boolean(
      selectedTask.value &&
        selectedTask.value.status === 'open' &&
        !selectedTaskDispatchDisabledReason.value &&
        selectedTaskLatestDispatch.value &&
        selectedTaskLatestDispatch.value.status !== 'preparing' &&
        selectedTaskLatestDispatch.value.status !== 'running',
    ),
  )

  const selectedTaskCanDiscardHistory = computed(() =>
    Boolean(
      selectedTask.value &&
        selectedTaskLatestDispatch.value &&
        selectedTaskLatestDispatch.value.status !== 'preparing' &&
        selectedTaskLatestDispatch.value.status !== 'running',
    ),
  )

  const selectedTaskLifecycleMessage = computed(() =>
    taskLifecycleProgressMessage(selectedTaskLifecycleMutation.value),
  )

  const selectedTaskPrimaryActionDisabled = computed(() =>
    Boolean(
      !selectedTask.value ||
        selectedTaskLifecycleMutation.value !== null ||
        options.dispatchingTaskId.value === selectedTask.value.id ||
        options.cancelingDispatchTaskId.value === selectedTask.value.id ||
        options.followingUpTaskId.value === selectedTask.value.id ||
        (
          selectedTask.value.status === 'open' &&
          selectedTaskLatestDispatch.value?.status !== 'preparing' &&
          selectedTaskLatestDispatch.value?.status !== 'running' &&
          !selectedTaskCanContinue.value &&
          Boolean(selectedTaskDispatchDisabledReason.value)
        ),
    ),
  )

  function selectTask(taskId: string) {
    selectedTaskId.value = taskId
    isTaskDrawerOpen.value = true

    if (options.currentPage.value !== 'tasks') {
      options.currentPage.value = 'tasks'
    }
  }

  function closeTaskDrawer() {
    isTaskDrawerOpen.value = false
    selectedTaskId.value = null
  }

  function openTaskFromRun(run: RunRecord) {
    options.currentPage.value = 'tasks'
    pendingSelectedTaskId.value = run.task.id
    isTaskDrawerOpen.value = true

    const needsProjectFilterChange = selectedProjectFilter.value !== run.task.project
    const needsClosedTasks = run.task.status === 'closed' && !showClosed.value

    selectedProjectFilter.value = run.task.project
    if (run.task.status === 'closed') {
      showClosed.value = true
    }

    if (!needsProjectFilterChange && !needsClosedTasks) {
      selectedTaskId.value = run.task.id
      pendingSelectedTaskId.value = null
    }
  }

  watch(
    options.tasks,
    (nextTasks) => {
      if (pendingSelectedTaskId.value) {
        const pendingTask = nextTasks.find((task) => task.id === pendingSelectedTaskId.value)
        if (pendingTask) {
          selectedTaskId.value = pendingTask.id
          pendingSelectedTaskId.value = null
          isTaskDrawerOpen.value = true
          return
        }
      }

      if (selectedTaskId.value && !nextTasks.some((task) => task.id === selectedTaskId.value)) {
        closeTaskDrawer()
      }
    },
    { immediate: true },
  )

  watch(
    selectedTaskId,
    () => {
      selectedTaskStartTool.value = options.defaultRemoteAgentPreferredTool.value
    },
    { immediate: true },
  )

  watch(options.defaultRemoteAgentPreferredTool, (nextTool, previousTool) => {
    if (selectedTaskStartTool.value === previousTool) {
      selectedTaskStartTool.value = nextTool
    }
  })

  watch(options.currentPage, (nextPage) => {
    if (nextPage !== 'tasks') {
      isTaskDrawerOpen.value = false
      options.selectedTaskRuns.value = []
    }
  })

  return {
    closeTaskDrawer,
    isTaskDrawerOpen,
    openTaskFromRun,
    pendingSelectedTaskId,
    selectTask,
    selectedProjectFilter,
    selectedTask,
    selectedTaskCanContinue,
    selectedTaskCanDiscardHistory,
    selectedTaskCanStartFresh,
    selectedTaskDispatchDisabledReason,
    selectedTaskDispatchTool,
    selectedTaskId,
    selectedTaskLatestDispatch,
    selectedTaskLatestReusablePullRequest,
    selectedTaskLifecycleMessage,
    selectedTaskLifecycleMutation,
    selectedTaskPinnedTool,
    selectedTaskPrimaryActionDisabled,
    selectedTaskProject,
    selectedTaskStartTool,
    showClosed,
  }
}
