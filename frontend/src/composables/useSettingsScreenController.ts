import type { ComputedRef, Ref } from 'vue'

import type {
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
  RemoteCleanupSummary,
  RemoteResetSummary,
} from '../types/task'
import type { PendingRunnerSetupRequest } from './useTaskMutations'

interface UseSettingsScreenControllerOptions {
  data: {
    activeRemoteWorkCount: ComputedRef<number>
    remoteAgentSettings: Ref<RemoteAgentSettings | null>
    runnerSetupReady: ComputedRef<boolean>
    shellPreludeHelpText: string
  }
  state: {
    cleaningUpRemoteArtifacts: Ref<boolean>
    cleanupPendingConfirmation: Ref<boolean>
    cleanupSummary: Ref<RemoteCleanupSummary | null>
    editingRemoteAgentSetup: Ref<boolean>
    resetPendingConfirmation: Ref<boolean>
    resettingRemoteWorkspace: Ref<boolean>
    resetSummary: Ref<RemoteResetSummary | null>
    saving: Ref<boolean>
    taskPendingRunnerSetup: Ref<PendingRunnerSetupRequest | null>
  }
  actions: {
    confirmRemoteCleanup: () => Promise<void>
    confirmRemoteReset: () => Promise<void>
    saveRemoteAgentSetup: (payload: RemoteAgentSettingsUpdateInput) => Promise<void>
  }
}

/**
 * Defines the settings screen as the owner of runner-setup and maintenance UI.
 *
 * The underlying refs still live in App.vue today, but this controller keeps
 * the screen's administrative actions grouped behind one named API.
 */
export function useSettingsScreenController(options: UseSettingsScreenControllerOptions) {
  return {
    activeRemoteWorkCount: options.data.activeRemoteWorkCount,
    cleaningUpRemoteArtifacts: options.state.cleaningUpRemoteArtifacts,
    cleanupPendingConfirmation: options.state.cleanupPendingConfirmation,
    cleanupSummary: options.state.cleanupSummary,
    confirmRemoteCleanup: options.actions.confirmRemoteCleanup,
    confirmRemoteReset: options.actions.confirmRemoteReset,
    editingRemoteAgentSetup: options.state.editingRemoteAgentSetup,
    remoteAgentSettings: options.data.remoteAgentSettings,
    resetPendingConfirmation: options.state.resetPendingConfirmation,
    resettingRemoteWorkspace: options.state.resettingRemoteWorkspace,
    resetSummary: options.state.resetSummary,
    runnerSetupReady: options.data.runnerSetupReady,
    saveRemoteAgentSetup: options.actions.saveRemoteAgentSetup,
    saving: options.state.saving,
    shellPreludeHelpText: options.data.shellPreludeHelpText,
    taskPendingRunnerSetup: options.state.taskPendingRunnerSetup,
  }
}

export type SettingsScreenController = ReturnType<typeof useSettingsScreenController>
