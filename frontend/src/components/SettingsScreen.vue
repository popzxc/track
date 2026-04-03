<script setup lang="ts">
import ConfirmDialog from './ConfirmDialog.vue'
import RemoteAgentSetupModal from './RemoteAgentSetupModal.vue'
import SettingsPage from './SettingsPage.vue'
import type { SettingsScreenController } from '../composables/useSettingsScreenController'

const props = defineProps<{
  active: boolean
  controller: SettingsScreenController
}>()

// Settings owns the runner-setup and maintenance overlays because they mutate
// backend-managed infrastructure rather than one page's local presentation.
function openRunnerSetup() {
  props.controller.taskPendingRunnerSetup.value = null
  props.controller.editingRemoteAgentSetup.value = true
}

function closeRunnerSetup() {
  props.controller.editingRemoteAgentSetup.value = false
  props.controller.taskPendingRunnerSetup.value = null
}

function openRemoteCleanupConfirmation() {
  props.controller.cleanupPendingConfirmation.value = true
}

function clearPendingRemoteCleanup() {
  props.controller.cleanupPendingConfirmation.value = false
}

function openRemoteResetConfirmation() {
  props.controller.resetPendingConfirmation.value = true
}

function clearPendingRemoteReset() {
  props.controller.resetPendingConfirmation.value = false
}
</script>

<template>
  <SettingsPage
    v-if="active"
    :active-remote-work-count="controller.activeRemoteWorkCount.value"
    :cleaning-up-remote-artifacts="controller.cleaningUpRemoteArtifacts.value"
    :cleanup-summary="controller.cleanupSummary.value"
    :remote-agent-settings="controller.remoteAgentSettings.value"
    :reset-summary="controller.resetSummary.value"
    :resetting-remote-workspace="controller.resettingRemoteWorkspace.value"
    :runner-setup-ready="controller.runnerSetupReady.value"
    :shell-prelude-help-text="controller.shellPreludeHelpText"
    @request-open-cleanup="openRemoteCleanupConfirmation"
    @request-open-reset="openRemoteResetConfirmation"
    @request-open-runner-setup="openRunnerSetup"
  />

  <RemoteAgentSetupModal
    :busy="controller.saving.value"
    :open="controller.editingRemoteAgentSetup.value"
    :required-for-dispatch="controller.taskPendingRunnerSetup.value !== null"
    :settings="controller.remoteAgentSettings.value"
    @cancel="closeRunnerSetup"
    @save="controller.saveRemoteAgentSetup"
  />

  <ConfirmDialog
    :busy="controller.cleaningUpRemoteArtifacts.value"
    confirm-busy-label="Cleaning up..."
    confirm-label="Run cleanup"
    confirm-variant="primary"
    description="Sweep the remote workspace and remove stale worktrees plus orphaned dispatch artifacts using the same rules as task close/delete."
    eyebrow="Maintenance action"
    :open="controller.cleanupPendingConfirmation.value"
    title="Clean up remote artifacts"
    @cancel="clearPendingRemoteCleanup"
    @confirm="controller.confirmRemoteCleanup"
  />

  <ConfirmDialog
    :busy="controller.resettingRemoteWorkspace.value"
    confirm-busy-label="Resetting..."
    confirm-label="Reset workspace"
    confirm-variant="danger"
    description="Delete the entire remote workspace managed by track and remove the remote projects registry. Local tasks and local dispatch history will stay intact, but the next dispatch will need to rebuild the remote environment from scratch."
    eyebrow="Destructive remote action"
    :open="controller.resetPendingConfirmation.value"
    title="Reset remote workspace"
    @cancel="clearPendingRemoteReset"
    @confirm="controller.confirmRemoteReset"
  />
</template>
