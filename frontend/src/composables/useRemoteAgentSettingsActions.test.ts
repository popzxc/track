import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import type { RemoteAgentPreferredTool } from '../api/types'
import * as apiClient from '../api/client'
import { buildRemoteAgentSettings, buildTask } from '../testing/factories'
import { TOOL_CONSTANTS } from '../testing/constants'
import { useRemoteAgentSettingsActions } from './useRemoteAgentSettingsActions'

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

function createRemoteAgentSettingsActionsHarness() {
  const cleaningUpRemoteArtifacts = ref(false)
  const cleanupPendingConfirmation = ref(false)
  const cleanupSummary = ref(null)
  const editingRemoteAgentSetup = ref(true)
  const errorMessage = ref('')
  const remoteAgentSettings = ref(null)
  const resetPendingConfirmation = ref(false)
  const resetSummary = ref(null)
  const resettingRemoteWorkspace = ref(false)
  const saving = ref(false)
  const taskPendingRunnerSetup = ref<{
    task: ReturnType<typeof buildTask>
    preferredTool: RemoteAgentPreferredTool
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
    actions: useRemoteAgentSettingsActions({
      cleaningUpRemoteArtifacts,
      cleanupPendingConfirmation,
      cleanupSummary,
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

describe('useRemoteAgentSettingsActions', () => {
  it('saves remote runner settings and resumes the queued dispatch intent', async () => {
    vi.useFakeTimers()
    const harness = createRemoteAgentSettingsActionsHarness()
    const queuedTask = buildTask()
    const savedSettings = buildRemoteAgentSettings(
      { shellPrelude: 'export PATH=/srv/tools:$PATH' },
      { preferredTool: TOOL_CONSTANTS.CLAUDE },
    )
    harness.taskPendingRunnerSetup.value = {
      task: queuedTask,
      preferredTool: TOOL_CONSTANTS.CLAUDE,
    }
    vi.spyOn(apiClient, 'updateRemoteAgentSettings').mockResolvedValue(savedSettings)

    await harness.actions.saveRemoteAgentSetup({
      preferredTool: TOOL_CONSTANTS.CLAUDE,
      shellPrelude: 'export PATH=/srv/tools:$PATH',
    })
    await vi.runAllTimersAsync()

    expect(harness.remoteAgentSettings.value).toEqual(savedSettings)
    expect(harness.editingRemoteAgentSetup.value).toBe(false)
    expect(harness.taskPendingRunnerSetup.value).toBeNull()
    expect(harness.resumeQueuedTaskDispatch).toHaveBeenCalledWith(queuedTask, TOOL_CONSTANTS.CLAUDE)
  })
})
