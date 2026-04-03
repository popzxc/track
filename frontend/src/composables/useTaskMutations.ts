import type { ComputedRef, Ref } from 'vue'

import {
  cancelDispatch,
  createTask,
  deleteTask,
  discardDispatch,
  dispatchTask,
  followUpTask,
  updateTask,
} from '../api/client'
import type {
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  Task,
  TaskCreateInput,
  TaskDispatch,
  TaskFollowUpInput,
} from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'
type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

export interface PendingRunnerSetupRequest {
  task: Task
  preferredTool: RemoteAgentPreferredTool
}

interface UseTaskMutationsOptions {
  cancelingDispatchTaskId: Ref<string | null>
  closeTaskDrawer: () => void
  creatingTask: Ref<boolean>
  currentPage: Ref<AppPage>
  discardingDispatchTaskId: Ref<string | null>
  dispatchingTaskId: Ref<string | null>
  editingRemoteAgentSetup: Ref<boolean>
  editingTask: Ref<Task | null>
  errorMessage: Ref<string>
  followingUpTask: Ref<Task | null>
  followingUpTaskId: Ref<string | null>
  isTaskDrawerOpen: Ref<boolean>
  loadRemoteAgentSettings: () => Promise<void>
  loadRuns: () => Promise<void>
  pendingSelectedTaskId: Ref<string | null>
  refreshAll: () => Promise<void>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  removeTaskRuns: (taskId: string) => void
  runnerSetupReady: ComputedRef<boolean>
  saving: Ref<boolean>
  selectedProjectFilter: Ref<string>
  selectedTask: ComputedRef<Task | null>
  selectedTaskCanContinue: ComputedRef<boolean>
  selectedTaskDispatchTool: ComputedRef<RemoteAgentPreferredTool>
  selectedTaskId: Ref<string | null>
  selectedTaskLatestDispatch: ComputedRef<TaskDispatch | null>
  setFriendlyError: (error: unknown) => void
  showClosed: Ref<boolean>
  taskLifecycleMutation: Ref<TaskLifecycleMutation | null>
  taskLifecycleMutationTaskId: Ref<string | null>
  taskPendingDeletion: Ref<Task | null>
  taskPendingRunnerSetup: Ref<PendingRunnerSetupRequest | null>
  upsertLatestTaskDispatch: (dispatch: TaskDispatch) => void
  upsertRunRecord: (task: Task, dispatch: TaskDispatch) => void
  upsertSelectedTaskRun: (task: Task, dispatch: TaskDispatch) => void
}

function beginTaskLifecycleMutation(
  taskId: string,
  mutation: TaskLifecycleMutation,
  taskLifecycleMutationTaskId: Ref<string | null>,
  taskLifecycleMutation: Ref<TaskLifecycleMutation | null>,
) {
  taskLifecycleMutationTaskId.value = taskId
  taskLifecycleMutation.value = mutation
}

function clearTaskLifecycleMutation(
  taskLifecycleMutationTaskId: Ref<string | null>,
  taskLifecycleMutation: Ref<TaskLifecycleMutation | null>,
) {
  taskLifecycleMutationTaskId.value = null
  taskLifecycleMutation.value = null
}

/**
 * Owns task-facing mutations and the UI intent that surrounds them.
 *
 * Task actions are not simple CRUD calls. Dispatch can redirect into runner
 * setup, creation needs to pre-select the freshly created task, and deletion
 * must keep drawer state in sync with the queue. Grouping that orchestration
 * here keeps `App.vue` focused on composition while preserving the shell's
 * conservative "refresh from persisted state after writes" policy.
 */
export function useTaskMutations(options: UseTaskMutationsOptions) {
  async function updateTaskStatus(task: Task, status: Task['status']) {
    options.saving.value = true
    options.errorMessage.value = ''
    beginTaskLifecycleMutation(
      task.id,
      status === 'closed' ? 'closing' : 'reopening',
      options.taskLifecycleMutationTaskId,
      options.taskLifecycleMutation,
    )

    try {
      await updateTask(task.id, { status })
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.saving.value = false
      clearTaskLifecycleMutation(options.taskLifecycleMutationTaskId, options.taskLifecycleMutation)
    }
  }

  async function saveTaskEdits(payload: { description: string; priority: Task['priority'] }) {
    if (!options.editingTask.value) {
      return
    }

    options.saving.value = true
    options.errorMessage.value = ''

    try {
      await updateTask(options.editingTask.value.id, payload)
      options.editingTask.value = null
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.saving.value = false
    }
  }

  async function createTaskFromWeb(payload: TaskCreateInput) {
    options.saving.value = true
    options.errorMessage.value = ''

    try {
      const task = await createTask(payload)

      // New tasks should land the user directly in the freshly created queue
      // context instead of leaving them in whichever filter or page they used
      // before opening the modal.
      options.creatingTask.value = false
      options.pendingSelectedTaskId.value = task.id
      options.isTaskDrawerOpen.value = true
      options.currentPage.value = 'tasks'
      options.selectedProjectFilter.value = task.project
      options.showClosed.value = false
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.saving.value = false
    }
  }

  async function confirmDelete() {
    if (!options.taskPendingDeletion.value) {
      return
    }

    options.saving.value = true
    options.errorMessage.value = ''
    beginTaskLifecycleMutation(
      options.taskPendingDeletion.value.id,
      'deleting',
      options.taskLifecycleMutationTaskId,
      options.taskLifecycleMutation,
    )

    try {
      const deletedTaskId = options.taskPendingDeletion.value.id
      await deleteTask(deletedTaskId)
      options.taskPendingDeletion.value = null

      if (options.selectedTaskId.value === deletedTaskId) {
        options.closeTaskDrawer()
      }

      options.removeTaskRuns(deletedTaskId)
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.saving.value = false
      clearTaskLifecycleMutation(options.taskLifecycleMutationTaskId, options.taskLifecycleMutation)
    }
  }

  async function startRemoteRun(
    task: Task,
    preferredTool: RemoteAgentPreferredTool = options.selectedTaskDispatchTool.value,
  ) {
    if (options.remoteAgentSettings.value === null) {
      try {
        await options.loadRemoteAgentSettings()
      } catch {
        // The user-facing message below remains the primary fallback if the
        // settings endpoint is still unavailable after a best-effort reload.
      }
    }

    if (options.remoteAgentSettings.value && !options.remoteAgentSettings.value.configured) {
      options.errorMessage.value =
        'Remote dispatch is not configured yet. Run `track remote-agent configure --host <host> --user <user> --identity-file ~/.ssh/track_remote_agent` locally first.'
      options.currentPage.value = 'settings'
      return
    }

    if (options.remoteAgentSettings.value && !options.runnerSetupReady.value) {
      options.taskPendingRunnerSetup.value = { task, preferredTool }
      options.editingRemoteAgentSetup.value = true
      options.currentPage.value = 'settings'
      return
    }

    options.dispatchingTaskId.value = task.id
    options.errorMessage.value = ''

    try {
      const dispatch = await dispatchTask(task.id, { preferredTool })
      options.upsertRunRecord(task, dispatch)
      options.upsertLatestTaskDispatch(dispatch)
      options.upsertSelectedTaskRun(task, dispatch)
    } catch (error) {
      await options.loadRuns().catch(() => undefined)
      options.setFriendlyError(error)
    } finally {
      options.dispatchingTaskId.value = null
    }
  }

  async function cancelRemoteRun(task: Task) {
    options.cancelingDispatchTaskId.value = task.id
    options.errorMessage.value = ''

    try {
      const dispatch = await cancelDispatch(task.id)
      options.upsertRunRecord(task, dispatch)
      options.upsertLatestTaskDispatch(dispatch)
      options.upsertSelectedTaskRun(task, dispatch)
    } catch (error) {
      await options.loadRuns().catch(() => undefined)
      options.setFriendlyError(error)
    } finally {
      options.cancelingDispatchTaskId.value = null
    }
  }

  async function discardRunHistory(task: Task) {
    options.discardingDispatchTaskId.value = task.id
    options.errorMessage.value = ''

    try {
      await discardDispatch(task.id)
      options.removeTaskRuns(task.id)
    } catch (error) {
      await options.loadRuns().catch(() => undefined)
      options.setFriendlyError(error)
    } finally {
      options.discardingDispatchTaskId.value = null
    }
  }

  async function submitFollowUp(payload: TaskFollowUpInput) {
    if (!options.followingUpTask.value) {
      return
    }

    options.followingUpTaskId.value = options.followingUpTask.value.id
    options.errorMessage.value = ''

    try {
      const task = options.followingUpTask.value
      const dispatch = await followUpTask(task.id, payload)
      options.upsertRunRecord(task, dispatch)
      options.upsertLatestTaskDispatch(dispatch)
      options.upsertSelectedTaskRun(task, dispatch)
      options.followingUpTask.value = null
      await options.refreshAll()
    } catch (error) {
      await options.loadRuns().catch(() => undefined)
      options.setFriendlyError(error)
    } finally {
      options.followingUpTaskId.value = null
    }
  }

  async function handlePrimaryAction() {
    if (!options.selectedTask.value) {
      return
    }

    const task = options.selectedTask.value
    const latestDispatch = options.selectedTaskLatestDispatch.value

    if (task.status === 'closed') {
      await updateTaskStatus(task, 'open')
      return
    }

    if (latestDispatch?.status === 'preparing' || latestDispatch?.status === 'running') {
      await cancelRemoteRun(task)
      return
    }

    if (options.selectedTaskCanContinue.value) {
      options.followingUpTask.value = task
      return
    }

    await startRemoteRun(task)
  }

  return {
    cancelRemoteRun,
    confirmDelete,
    discardRunHistory,
    handlePrimaryAction,
    saveTaskEdits,
    startRemoteRun,
    submitFollowUp,
    updateTaskStatus,
    createTaskFromWeb,
  }
}
