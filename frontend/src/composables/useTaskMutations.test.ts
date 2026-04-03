import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'

import * as apiClient from '../api/client'
import { buildDispatch, buildTask } from '../testing/factories'
import { useTaskMutations } from './useTaskMutations'

afterEach(() => {
  vi.restoreAllMocks()
})

function createTaskMutationHarness() {
  const task = buildTask()
  const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('tasks')
  const creatingTask = ref(false)
  const editingTask = ref<ReturnType<typeof buildTask> | null>(null)
  const editingRemoteAgentSetup = ref(false)
  const errorMessage = ref('')
  const followingUpTask = ref<ReturnType<typeof buildTask> | null>(null)
  const followingUpTaskId = ref<string | null>(null)
  const isTaskDrawerOpen = ref(false)
  const pendingSelectedTaskId = ref<string | null>(null)
  const remoteAgentSettings = ref({
    configured: true,
    preferredTool: 'codex' as const,
    host: '127.0.0.1',
    user: 'track',
    port: 22,
    shellPrelude: 'export PATH=/opt/bin:$PATH',
    reviewFollowUp: {
      enabled: false,
      mainUser: undefined,
      defaultReviewPrompt: undefined,
    },
  })
  const runnerSetupReady = ref(true)
  const saving = ref(false)
  const selectedProjectFilter = ref('')
  const selectedTaskRef = ref(task)
  const selectedTaskCanContinue = ref(false)
  const selectedTaskDispatchTool = ref<'codex' | 'claude'>('codex')
  const selectedTaskId = ref<string | null>(task.id)
  const selectedTaskLatestDispatch = ref<ReturnType<typeof buildDispatch> | null>(null)
  const showClosed = ref(false)
  const taskLifecycleMutation = ref<'closing' | 'reopening' | 'deleting' | null>(null)
  const taskLifecycleMutationTaskId = ref<string | null>(null)
  const taskPendingDeletion = ref<ReturnType<typeof buildTask> | null>(null)
  const taskPendingRunnerSetup = ref<{
    task: ReturnType<typeof buildTask>
    preferredTool: 'codex' | 'claude'
  } | null>(null)
  const cancelingDispatchTaskId = ref<string | null>(null)
  const discardingDispatchTaskId = ref<string | null>(null)
  const dispatchingTaskId = ref<string | null>(null)

  const closeTaskDrawer = vi.fn()
  const loadRemoteAgentSettings = vi.fn(async () => undefined)
  const loadRuns = vi.fn(async () => undefined)
  const refreshAll = vi.fn(async () => undefined)
  const removeTaskRuns = vi.fn()
  const setFriendlyError = vi.fn()
  const upsertLatestTaskDispatch = vi.fn()
  const upsertRunRecord = vi.fn()
  const upsertSelectedTaskRun = vi.fn()

  return {
    task,
    closeTaskDrawer,
    creatingTask,
    currentPage,
    dispatchingTaskId,
    editingRemoteAgentSetup,
    loadRemoteAgentSettings,
    loadRuns,
    refreshAll,
    remoteAgentSettings,
    runnerSetupReady,
    selectedTaskId,
    taskLifecycleMutation,
    taskLifecycleMutationTaskId,
    taskPendingDeletion,
    taskPendingRunnerSetup,
    removeTaskRuns,
    upsertLatestTaskDispatch,
    upsertRunRecord,
    upsertSelectedTaskRun,
    mutations: useTaskMutations({
      cancelingDispatchTaskId,
      closeTaskDrawer,
      creatingTask,
      currentPage,
      discardingDispatchTaskId,
      dispatchingTaskId,
      editingRemoteAgentSetup,
      editingTask,
      errorMessage,
      followingUpTask,
      followingUpTaskId,
      isTaskDrawerOpen,
      loadRemoteAgentSettings,
      loadRuns,
      pendingSelectedTaskId,
      refreshAll,
      remoteAgentSettings,
      removeTaskRuns,
      runnerSetupReady: computed(() => runnerSetupReady.value),
      saving,
      selectedProjectFilter,
      selectedTask: computed(() => selectedTaskRef.value),
      selectedTaskCanContinue: computed(() => selectedTaskCanContinue.value),
      selectedTaskDispatchTool: computed(() => selectedTaskDispatchTool.value),
      selectedTaskId,
      selectedTaskLatestDispatch: computed(() => selectedTaskLatestDispatch.value),
      setFriendlyError,
      showClosed,
      taskLifecycleMutation,
      taskLifecycleMutationTaskId,
      taskPendingDeletion,
      taskPendingRunnerSetup,
      upsertLatestTaskDispatch,
      upsertRunRecord,
      upsertSelectedTaskRun,
    }),
  }
}

describe('useTaskMutations', () => {
  it('opens runner setup instead of dispatching when the remote shell prelude is missing', async () => {
    const harness = createTaskMutationHarness()
    harness.runnerSetupReady.value = false
    const dispatchTaskSpy = vi.spyOn(apiClient, 'dispatchTask')

    await harness.mutations.startRemoteRun(harness.task, 'claude')

    expect(harness.currentPage.value).toBe('settings')
    expect(harness.editingRemoteAgentSetup.value).toBe(true)
    expect(harness.taskPendingRunnerSetup.value).toEqual({
      task: harness.task,
      preferredTool: 'claude',
    })
    expect(dispatchTaskSpy).not.toHaveBeenCalled()
  })

  it('updates the visible run projections after a successful dispatch', async () => {
    const harness = createTaskMutationHarness()
    const dispatch = buildDispatch({
      dispatchId: 'dispatch-new',
      taskId: harness.task.id,
      project: harness.task.project,
      preferredTool: 'claude',
    })
    const dispatchTaskSpy = vi.spyOn(apiClient, 'dispatchTask').mockResolvedValue(dispatch)

    await harness.mutations.startRemoteRun(harness.task, 'claude')

    expect(dispatchTaskSpy).toHaveBeenCalledWith(harness.task.id, { preferredTool: 'claude' })
    expect(harness.upsertRunRecord).toHaveBeenCalledWith(harness.task, dispatch)
    expect(harness.upsertLatestTaskDispatch).toHaveBeenCalledWith(dispatch)
    expect(harness.upsertSelectedTaskRun).toHaveBeenCalledWith(harness.task, dispatch)
    expect(harness.dispatchingTaskId.value).toBeNull()
  })

  it('deletes the selected task and clears its local drawer state', async () => {
    const harness = createTaskMutationHarness()
    harness.taskPendingDeletion.value = harness.task
    const deleteTaskSpy = vi.spyOn(apiClient, 'deleteTask').mockResolvedValue({ ok: true })

    await harness.mutations.confirmDelete()

    expect(deleteTaskSpy).toHaveBeenCalledWith(harness.task.id)
    expect(harness.closeTaskDrawer).toHaveBeenCalledTimes(1)
    expect(harness.removeTaskRuns).toHaveBeenCalledWith(harness.task.id)
    expect(harness.refreshAll).toHaveBeenCalledTimes(1)
    expect(harness.taskPendingDeletion.value).toBeNull()
    expect(harness.taskLifecycleMutation.value).toBeNull()
    expect(harness.taskLifecycleMutationTaskId.value).toBeNull()
  })
})
