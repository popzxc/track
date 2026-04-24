import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'

import type { RemoteAgentPreferredTool } from '../api/types'
import * as apiClient from '../api/client'
import { buildDispatch, buildDispatchForTool, buildTask } from '../testing/factories'
import { ALL_TOOLS, TOOL_CONSTANTS } from '../testing/constants'
import { useTaskMutations } from './useTaskMutations'

afterEach(() => {
  vi.restoreAllMocks()
})

function createTaskMutationHarness() {
  const task = buildTask()
  const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('tasks')
  const creatingTask = ref(false)
  const editingTask = ref<ReturnType<typeof buildTask> | null>(null)
  const errorMessage = ref('')
  const followingUpTask = ref<ReturnType<typeof buildTask> | null>(null)
  const followingUpTaskId = ref<string | null>(null)
  const isTaskDrawerOpen = ref(false)
  const pendingSelectedTaskId = ref<string | null>(null)
  const remoteAgentSettings = ref({
    configured: true,
    preferredTool: TOOL_CONSTANTS.CODEX,
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
  const selectedTaskDispatchTool = ref<RemoteAgentPreferredTool>(TOOL_CONSTANTS.CODEX)
  const selectedTaskId = ref<string | null>(task.id)
  const selectedTaskLatestDispatch = ref<ReturnType<typeof buildDispatch> | null>(null)
  const showClosed = ref(false)
  const taskLifecycleMutation = ref<'closing' | 'reopening' | 'deleting' | null>(null)
  const taskLifecycleMutationTaskId = ref<string | null>(null)
  const taskPendingDeletion = ref<ReturnType<typeof buildTask> | null>(null)
  const cancelingDispatchTaskId = ref<string | null>(null)
  const discardingDispatchTaskId = ref<string | null>(null)
  const dispatchingTaskId = ref<string | null>(null)

  const closeTaskDrawer = vi.fn()
  const loadRemoteAgentSettings = vi.fn(async () => undefined)
  const loadRuns = vi.fn(async () => undefined)
  const refreshAll = vi.fn(async () => undefined)
  const removeTaskRuns = vi.fn()
  const requestRunnerSetup = vi.fn((queuedTask: ReturnType<typeof buildTask>, preferredTool: RemoteAgentPreferredTool) => {
    currentPage.value = 'settings'
    runnerSetupRequests.value.push({ task: queuedTask, preferredTool })
  })
  const runnerSetupRequests = ref<Array<{
    task: ReturnType<typeof buildTask>
    preferredTool: RemoteAgentPreferredTool
  }>>([])
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
    loadRemoteAgentSettings,
    loadRuns,
    refreshAll,
    remoteAgentSettings,
    requestRunnerSetup,
    runnerSetupReady,
    runnerSetupRequests,
    selectedTaskId,
    taskLifecycleMutation,
    taskLifecycleMutationTaskId,
    taskPendingDeletion,
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
      requestRunnerSetup,
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

    await harness.mutations.startRemoteRun(harness.task, TOOL_CONSTANTS.CLAUDE)

    expect(harness.currentPage.value).toBe('settings')
    expect(harness.requestRunnerSetup).toHaveBeenCalledTimes(1)
    expect(harness.runnerSetupRequests.value).toEqual([{
      task: harness.task,
      preferredTool: TOOL_CONSTANTS.CLAUDE,
    }])
    expect(dispatchTaskSpy).not.toHaveBeenCalled()
  })

  it('updates the visible run projections after a successful dispatch', async () => {
    const harness = createTaskMutationHarness()
    const dispatch = buildDispatch(
      {
        dispatchId: 'dispatch-new',
        taskId: harness.task.id,
        project: harness.task.project,
      },
      { preferredTool: TOOL_CONSTANTS.CLAUDE },
    )
    const dispatchTaskSpy = vi.spyOn(apiClient, 'dispatchTask').mockResolvedValue(dispatch)

    await harness.mutations.startRemoteRun(harness.task, TOOL_CONSTANTS.CLAUDE)

    expect(dispatchTaskSpy).toHaveBeenCalledWith(harness.task.id, { preferredTool: TOOL_CONSTANTS.CLAUDE })
    expect(harness.upsertRunRecord).toHaveBeenCalledWith(harness.task, dispatch)
    expect(harness.upsertLatestTaskDispatch).toHaveBeenCalledWith(dispatch)
    expect(harness.upsertSelectedTaskRun).toHaveBeenCalledWith(harness.task, dispatch)
    expect(harness.dispatchingTaskId.value).toBeNull()
  })

  describe.each(ALL_TOOLS)('tool support (%s)', (tool) => {
    it(`dispatches with ${tool}`, async () => {
      const harness = createTaskMutationHarness()
      const dispatch = buildDispatchForTool(tool)
      const dispatchTaskSpy = vi.spyOn(apiClient, 'dispatchTask').mockResolvedValue(dispatch)

      await harness.mutations.startRemoteRun(harness.task, tool)

      expect(dispatchTaskSpy).toHaveBeenCalledWith(harness.task.id, { preferredTool: tool })
      expect(harness.upsertRunRecord).toHaveBeenCalledWith(harness.task, dispatch)
    })

    it(`opens runner setup for ${tool} when not ready`, async () => {
      const harness = createTaskMutationHarness()
      harness.runnerSetupReady.value = false
      const dispatchTaskSpy = vi.spyOn(apiClient, 'dispatchTask')

      await harness.mutations.startRemoteRun(harness.task, tool)

      expect(harness.currentPage.value).toBe('settings')
      expect(harness.requestRunnerSetup).toHaveBeenCalledWith(harness.task, tool)
      expect(dispatchTaskSpy).not.toHaveBeenCalled()
    })
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
