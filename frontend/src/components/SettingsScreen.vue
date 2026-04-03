<script setup lang="ts">
import type { ComputedRef, Ref } from 'vue'

import ConfirmDialog from './ConfirmDialog.vue'
import RemoteAgentSetupModal from './RemoteAgentSetupModal.vue'
import SettingsPage from './SettingsPage.vue'
import type {
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
  RemoteCleanupSummary,
  RemoteResetSummary,
} from '../types/task'
import type { PendingRunnerSetupRequest } from '../composables/useTaskMutations'

interface SettingsScreenContext {
  activeRemoteWorkCount: ComputedRef<number>
  cleaningUpRemoteArtifacts: Ref<boolean>
  cleanupPendingConfirmation: Ref<boolean>
  cleanupSummary: Ref<RemoteCleanupSummary | null>
  confirmRemoteCleanup: () => Promise<void>
  confirmRemoteReset: () => Promise<void>
  editingRemoteAgentSetup: Ref<boolean>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  resetPendingConfirmation: Ref<boolean>
  resettingRemoteWorkspace: Ref<boolean>
  resetSummary: Ref<RemoteResetSummary | null>
  runnerSetupReady: ComputedRef<boolean>
  saveRemoteAgentSetup: (payload: RemoteAgentSettingsUpdateInput) => Promise<void>
  saving: Ref<boolean>
  shellPreludeHelpText: string
  taskPendingRunnerSetup: Ref<PendingRunnerSetupRequest | null>
}

const props = defineProps<{
  active: boolean
  context: SettingsScreenContext
}>()

// Settings owns the runner-setup and maintenance overlays because they mutate
// backend-managed infrastructure rather than one page's local presentation.
function openRunnerSetup() {
  props.context.taskPendingRunnerSetup.value = null
  props.context.editingRemoteAgentSetup.value = true
}

function closeRunnerSetup() {
  props.context.editingRemoteAgentSetup.value = false
  props.context.taskPendingRunnerSetup.value = null
}

function openRemoteCleanupConfirmation() {
  props.context.cleanupPendingConfirmation.value = true
}

function clearPendingRemoteCleanup() {
  props.context.cleanupPendingConfirmation.value = false
}

function openRemoteResetConfirmation() {
  props.context.resetPendingConfirmation.value = true
}

function clearPendingRemoteReset() {
  props.context.resetPendingConfirmation.value = false
}
</script>

<template>
  <SettingsPage
    v-if="active"
    :active-remote-work-count="context.activeRemoteWorkCount.value"
    :cleaning-up-remote-artifacts="context.cleaningUpRemoteArtifacts.value"
    :cleanup-summary="context.cleanupSummary.value"
    :remote-agent-settings="context.remoteAgentSettings.value"
    :reset-summary="context.resetSummary.value"
    :resetting-remote-workspace="context.resettingRemoteWorkspace.value"
    :runner-setup-ready="context.runnerSetupReady.value"
    :shell-prelude-help-text="context.shellPreludeHelpText"
    @request-open-cleanup="openRemoteCleanupConfirmation"
    @request-open-reset="openRemoteResetConfirmation"
    @request-open-runner-setup="openRunnerSetup"
  />

  <RemoteAgentSetupModal
    :busy="context.saving.value"
    :open="context.editingRemoteAgentSetup.value"
    :required-for-dispatch="context.taskPendingRunnerSetup.value !== null"
    :settings="context.remoteAgentSettings.value"
    @cancel="closeRunnerSetup"
    @save="context.saveRemoteAgentSetup"
  />

  <ConfirmDialog
    :busy="context.cleaningUpRemoteArtifacts.value"
    confirm-busy-label="Cleaning up..."
    confirm-label="Run cleanup"
    confirm-variant="primary"
    description="Sweep the remote workspace and remove stale worktrees plus orphaned dispatch artifacts using the same rules as task close/delete."
    eyebrow="Maintenance action"
    :open="context.cleanupPendingConfirmation.value"
    title="Clean up remote artifacts"
    @cancel="clearPendingRemoteCleanup"
    @confirm="context.confirmRemoteCleanup"
  />

  <ConfirmDialog
    :busy="context.resettingRemoteWorkspace.value"
    confirm-busy-label="Resetting..."
    confirm-label="Reset workspace"
    confirm-variant="danger"
    description="Delete the entire remote workspace managed by track and remove the remote projects registry. Local tasks and local dispatch history will stay intact, but the next dispatch will need to rebuild the remote environment from scratch."
    eyebrow="Destructive remote action"
    :open="context.resetPendingConfirmation.value"
    title="Reset remote workspace"
    @cancel="clearPendingRemoteReset"
    @confirm="context.confirmRemoteReset"
  />
</template>
