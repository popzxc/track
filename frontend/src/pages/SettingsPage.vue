<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import ConfirmDialog from '../components/ConfirmDialog.vue'
import RemoteAgentSetupModal from '../components/RemoteAgentSetupModal.vue'
import SettingsPageContent from '../components/SettingsPage.vue'
import { useSettingsMutations } from '../composables/useSettingsMutations'
import { useTrackerShell } from '../composables/useTrackerShell'
import { firstQueryValue, replaceRouteQuery } from '../router/query'
import type {
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteCleanupSummary,
  RemoteResetSummary,
  Task,
} from '../types/task'

const route = useRoute()
const router = useRouter()
const shell = useTrackerShell()

const cleanupSummary = ref<RemoteCleanupSummary | null>(null)
const resetSummary = ref<RemoteResetSummary | null>(null)
const cleaningUpRemoteArtifacts = ref(false)
const resettingRemoteWorkspace = ref(false)
const editingProject = ref<ProjectInfo | null>(null)

const modal = computed(() => firstQueryValue(route.query.modal))
const resumeTaskId = computed(() => firstQueryValue(route.query.resumeTask))
const resumePreferredTool = computed<RemoteAgentPreferredTool>(() => {
  const preferredTool = firstQueryValue(route.query.preferredTool)
  // TODO: Move `TOOL_CONSTANTS` from tests to actual types and use it instead.
  if (preferredTool === 'claude' || preferredTool === 'opencode' || preferredTool === 'codex') {
    return preferredTool
  }

  return 'codex'
})

const editingRemoteAgentSetup = computed<boolean>({
  get: () => modal.value === 'runner-setup',
  set: (open) => {
    void replaceRouteQuery(router, route, { modal: open ? 'runner-setup' : null })
  },
})

const cleanupPendingConfirmation = computed<boolean>({
  get: () => modal.value === 'cleanup',
  set: (open) => {
    void replaceRouteQuery(router, route, { modal: open ? 'cleanup' : null })
  },
})

const resetPendingConfirmation = computed<boolean>({
  get: () => modal.value === 'reset',
  set: (open) => {
    void replaceRouteQuery(router, route, { modal: open ? 'reset' : null })
  },
})

const taskPendingRunnerSetup = computed<{
  task: Task
  preferredTool: RemoteAgentPreferredTool
} | null>({
  get: () => {
    if (!resumeTaskId.value) {
      return null
    }

    const task = shell.tasks.value.find((candidate) => candidate.id === resumeTaskId.value)
    if (!task) {
      return null
    }

    return {
      task,
      preferredTool: resumePreferredTool.value,
    }
  },
  set: (request) => {
    void replaceRouteQuery(router, route, {
      modal: request ? 'runner-setup' : null,
      preferredTool: request?.preferredTool,
      resumeTask: request?.task.id,
    })
  },
})

const {
  confirmRemoteCleanup,
  confirmRemoteReset,
  saveRemoteAgentSetup,
} = useSettingsMutations({
  cleaningUpRemoteArtifacts,
  cleanupPendingConfirmation,
  cleanupSummary,
  editingProject,
  editingRemoteAgentSetup,
  errorMessage: shell.errorMessage,
  refreshAll: shell.refreshAll,
  remoteAgentSettings: shell.remoteAgentSettings,
  resetPendingConfirmation,
  resetSummary,
  resettingRemoteWorkspace,
  resumeQueuedTaskDispatch(task, preferredTool) {
    void router.push({
      name: 'tasks',
      query: {
        project: task.project,
        task: task.id,
      },
    })

    void shell.startQueuedTaskDispatch(task, preferredTool).catch(shell.setFriendlyError)
  },
  saving: shell.saving,
  setFriendlyError: shell.setFriendlyError,
  taskPendingRunnerSetup,
})

function openRunnerSetup() {
  taskPendingRunnerSetup.value = null
  editingRemoteAgentSetup.value = true
}

function closeRunnerSetup() {
  editingRemoteAgentSetup.value = false
  taskPendingRunnerSetup.value = null
}

function openRemoteCleanupConfirmation() {
  cleanupPendingConfirmation.value = true
}

function clearPendingRemoteCleanup() {
  cleanupPendingConfirmation.value = false
}

function openRemoteResetConfirmation() {
  resetPendingConfirmation.value = true
}

function clearPendingRemoteReset() {
  resetPendingConfirmation.value = false
}
</script>

<template>
  <SettingsPageContent
    :active-remote-work-count="shell.activeRemoteWorkCount.value"
    :cleaning-up-remote-artifacts="cleaningUpRemoteArtifacts"
    :cleanup-summary="cleanupSummary"
    :remote-agent-settings="shell.remoteAgentSettings.value"
    :reset-summary="resetSummary"
    :resetting-remote-workspace="resettingRemoteWorkspace"
    :runner-setup-ready="shell.runnerSetupReady.value"
    :shell-prelude-help-text="shell.shellPreludeHelpText"
    @request-open-cleanup="openRemoteCleanupConfirmation"
    @request-open-reset="openRemoteResetConfirmation"
    @request-open-runner-setup="openRunnerSetup"
  />

  <RemoteAgentSetupModal
    :busy="shell.saving.value"
    :open="editingRemoteAgentSetup"
    :required-for-dispatch="taskPendingRunnerSetup !== null"
    :settings="shell.remoteAgentSettings.value"
    @cancel="closeRunnerSetup"
    @save="saveRemoteAgentSetup"
  />

  <ConfirmDialog
    :busy="cleaningUpRemoteArtifacts"
    confirm-busy-label="Cleaning up..."
    confirm-label="Run cleanup"
    confirm-variant="primary"
    description="Sweep the remote workspace and remove stale worktrees plus orphaned dispatch artifacts using the same rules as task close/delete."
    eyebrow="Maintenance action"
    :open="cleanupPendingConfirmation"
    title="Clean up remote artifacts"
    @cancel="clearPendingRemoteCleanup"
    @confirm="confirmRemoteCleanup"
  />

  <ConfirmDialog
    :busy="resettingRemoteWorkspace"
    confirm-busy-label="Resetting..."
    confirm-label="Reset workspace"
    confirm-variant="danger"
    description="Delete the entire remote workspace managed by track and remove the remote projects registry. Local tasks and local dispatch history will stay intact, but the next dispatch will need to rebuild the remote environment from scratch."
    eyebrow="Destructive remote action"
    :open="resetPendingConfirmation"
    title="Reset remote workspace"
    @cancel="clearPendingRemoteReset"
    @confirm="confirmRemoteReset"
  />
</template>
