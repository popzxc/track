import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import * as apiClient from '../api/client'
import { buildRemoteAgentSettings, buildTask } from '../testing/factories'
import { useSettingsMutations } from './useSettingsMutations'

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

function createSettingsMutationHarness() {
  const cleaningUpRemoteArtifacts = ref(false)
  const cleanupPendingConfirmation = ref(false)
  const cleanupSummary = ref(null)
  const editingProject = ref(null)
  const editingRemoteAgentSetup = ref(true)
  const errorMessage = ref('')
  const remoteAgentSettings = ref(null)
  const resetPendingConfirmation = ref(false)
  const resetSummary = ref(null)
  const resettingRemoteWorkspace = ref(false)
  const saving = ref(false)
  const taskPendingRunnerSetup = ref<{
    task: ReturnType<typeof buildTask>
    preferredTool: 'codex' | 'claude'
  } | null>(null)

  const refreshAll = vi.fn(async () => undefined)
  const resumeQueuedTaskDispatch = vi.fn()
  const setFriendlyError = vi.fn()

  return {
    editingRemoteAgentSetup,
    refreshAll,
    remoteAgentSettings,
    resumeQueuedTaskDispatch,
    taskPendingRunnerSetup,
    mutations: useSettingsMutations({
      cleaningUpRemoteArtifacts,
      cleanupPendingConfirmation,
      cleanupSummary,
      editingProject,
      editingRemoteAgentSetup,
      errorMessage,
      refreshAll,
      remoteAgentSettings,
      resetPendingConfirmation,
      resetSummary,
      resettingRemoteWorkspace,
      resumeQueuedTaskDispatch,
      saving,
      setFriendlyError,
      taskPendingRunnerSetup,
    }),
  }
}

describe('useSettingsMutations', () => {
  it('saves remote runner settings and resumes the queued dispatch intent', async () => {
    vi.useFakeTimers()
    const harness = createSettingsMutationHarness()
    const queuedTask = buildTask()
    const savedSettings = buildRemoteAgentSettings({
      preferredTool: 'claude',
      shellPrelude: 'export PATH=/srv/tools:$PATH',
    })
    harness.taskPendingRunnerSetup.value = {
      task: queuedTask,
      preferredTool: 'claude',
    }
    vi.spyOn(apiClient, 'updateRemoteAgentSettings').mockResolvedValue(savedSettings)

    await harness.mutations.saveRemoteAgentSetup({
      preferredTool: 'claude',
      shellPrelude: 'export PATH=/srv/tools:$PATH',
    })
    await vi.runAllTimersAsync()

    expect(harness.remoteAgentSettings.value).toEqual(savedSettings)
    expect(harness.editingRemoteAgentSetup.value).toBe(false)
    expect(harness.taskPendingRunnerSetup.value).toBeNull()
    expect(harness.resumeQueuedTaskDispatch).toHaveBeenCalledWith(queuedTask, 'claude')
  })
})
