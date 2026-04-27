import type { Ref } from 'vue'

import {
  cleanupRemoteAgentArtifacts,
  resetRemoteAgentWorkspace,
  updateRemoteAgentSettings,
} from '../api/client'
import type {
  RemoteCleanupSummary,
  RemoteResetSummary,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
  Task,
} from '../types/task'

interface PendingRunnerSetupRequest {
  task: Task
  preferredTool: RemoteAgentPreferredTool
}

interface UseSettingsMutationsOptions {
  cleaningUpRemoteArtifacts: Ref<boolean>
  cleanupPendingConfirmation: Ref<boolean>
  cleanupSummary: Ref<RemoteCleanupSummary | null>
  editingRemoteAgentSetup: Ref<boolean>
  errorMessage: Ref<string>
  refreshAll: () => Promise<void>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  resetPendingConfirmation: Ref<boolean>
  resetSummary: Ref<RemoteResetSummary | null>
  resettingRemoteWorkspace: Ref<boolean>
  resumeQueuedTaskDispatch: (task: Task, preferredTool: RemoteAgentPreferredTool) => void
  saving: Ref<boolean>
  setFriendlyError: (error: unknown) => void
  taskPendingRunnerSetup: Ref<PendingRunnerSetupRequest | null>
}

/**
 * Owns administrative mutations that reshape the shell's environment.
 *
 * These actions do more than update one record. Remote setup can unblock a
 * queued dispatch, and cleanup/reset mutate backend-managed infrastructure.
 * Keeping them together highlights that they are "environment" changes rather
 * than everyday queue interactions.
 */
export function useSettingsMutations(options: UseSettingsMutationsOptions) {
  async function saveRemoteAgentSetup(payload: RemoteAgentSettingsUpdateInput) {
    options.saving.value = true
    options.errorMessage.value = ''

    try {
      options.remoteAgentSettings.value = await updateRemoteAgentSettings(payload)
      options.editingRemoteAgentSetup.value = false

      const queuedTask = options.taskPendingRunnerSetup.value
      options.taskPendingRunnerSetup.value = null

      if (queuedTask) {
        // The settings modal closes first so the resumed dispatch feels like a
        // continuation of the original intent rather than a second explicit
        // action the user needs to take.
        window.setTimeout(() => {
          options.resumeQueuedTaskDispatch(queuedTask.task, queuedTask.preferredTool)
        }, 0)
      }
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.saving.value = false
    }
  }

  async function confirmRemoteCleanup() {
    options.cleaningUpRemoteArtifacts.value = true
    options.errorMessage.value = ''

    try {
      options.cleanupSummary.value = await cleanupRemoteAgentArtifacts()
      options.cleanupPendingConfirmation.value = false
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.cleaningUpRemoteArtifacts.value = false
    }
  }

  async function confirmRemoteReset() {
    options.resettingRemoteWorkspace.value = true
    options.errorMessage.value = ''

    try {
      options.resetSummary.value = await resetRemoteAgentWorkspace()
      options.resetPendingConfirmation.value = false
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.resettingRemoteWorkspace.value = false
    }
  }

  return {
    confirmRemoteCleanup,
    confirmRemoteReset,
    saveRemoteAgentSetup,
  }
}
