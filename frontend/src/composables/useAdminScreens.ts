import { computed, ref, type ComputedRef, type Ref } from 'vue'

import { useProjectsScreenController } from './useProjectsScreenController'
import { useProjectViewState } from './useProjectViewState'
import { useSettingsMutations } from './useSettingsMutations'
import { useSettingsScreenController } from './useSettingsScreenController'
import type { PendingRunnerSetupRequest } from './useTaskMutations'
import type {
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RemoteCleanupSummary,
  RemoteResetSummary,
  Task,
} from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface UseAdminScreensOptions {
  activeRemoteWorkCount: ComputedRef<number>
  availableProjects: ComputedRef<ProjectInfo[]>
  closeTaskDrawer: () => void
  currentPage: Ref<AppPage>
  errorMessage: Ref<string>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  refreshAll: () => Promise<void>
  resumeQueuedTaskDispatch: (task: Task, preferredTool: RemoteAgentPreferredTool) => void
  runnerSetupReady: ComputedRef<boolean>
  saving: Ref<boolean>
  selectedProjectFilter: Ref<string>
  setFriendlyError: (error: unknown) => void
  shellPreludeHelpText: string
}

/**
 * Owns the administrative screens that coordinate infrastructure and metadata.
 *
 * Projects and Settings both mutate durable environment state rather than the
 * queue's ephemeral view state. Grouping them together highlights the cross-
 * screen behaviors that are intentional here: selecting a task's project opens
 * project metadata, and saving runner setup can resume work that originated in
 * the task screen.
 */
export function useAdminScreens(options: UseAdminScreensOptions) {
  const editingProject = ref<ProjectInfo | null>(null)
  const editingRemoteAgentSetup = ref(false)
  const taskPendingRunnerSetup = ref<PendingRunnerSetupRequest | null>(null)
  const cleanupPendingConfirmation = ref(false)
  const cleaningUpRemoteArtifacts = ref(false)
  const cleanupSummary = ref<RemoteCleanupSummary | null>(null)
  const resetPendingConfirmation = ref(false)
  const resettingRemoteWorkspace = ref(false)
  const resetSummary = ref<RemoteResetSummary | null>(null)

  const {
    selectProjectDetails,
    selectedProjectDetails,
    selectedProjectDetailsId,
  } = useProjectViewState({
    availableProjects: options.availableProjects,
    closeTaskDrawer: options.closeTaskDrawer,
    currentPage: options.currentPage,
    selectedProjectFilter: options.selectedProjectFilter,
  })

  const {
    confirmRemoteCleanup,
    confirmRemoteReset,
    saveProjectEdits,
    saveRemoteAgentSetup,
  } = useSettingsMutations({
    cleaningUpRemoteArtifacts,
    cleanupPendingConfirmation,
    cleanupSummary,
    editingProject,
    editingRemoteAgentSetup,
    errorMessage: options.errorMessage,
    refreshAll: options.refreshAll,
    remoteAgentSettings: options.remoteAgentSettings,
    resetPendingConfirmation,
    resetSummary,
    resettingRemoteWorkspace,
    resumeQueuedTaskDispatch(task, preferredTool) {
      options.resumeQueuedTaskDispatch(task, preferredTool)
    },
    saving: options.saving,
    setFriendlyError: options.setFriendlyError,
    taskPendingRunnerSetup,
  })

  const projectsScreen = useProjectsScreenController({
    availableProjects: options.availableProjects,
    editingProject,
    saveProjectEdits,
    saving: options.saving,
    selectedProjectDetails,
    selectedProjectDetailsId,
  })

  const settingsScreen = useSettingsScreenController({
    actions: {
      confirmRemoteCleanup,
      confirmRemoteReset,
      saveRemoteAgentSetup,
    },
    data: {
      activeRemoteWorkCount: options.activeRemoteWorkCount,
      remoteAgentSettings: options.remoteAgentSettings,
      runnerSetupReady: options.runnerSetupReady,
      shellPreludeHelpText: options.shellPreludeHelpText,
    },
    state: {
      cleaningUpRemoteArtifacts,
      cleanupPendingConfirmation,
      cleanupSummary,
      editingRemoteAgentSetup,
      resetPendingConfirmation,
      resettingRemoteWorkspace,
      resetSummary,
      saving: options.saving,
      taskPendingRunnerSetup,
    },
  })

  function requestProjectDetails(project: ProjectInfo) {
    selectProjectDetails(project)
  }

  function requestRunnerSetup(task: Task, preferredTool: RemoteAgentPreferredTool) {
    taskPendingRunnerSetup.value = { task, preferredTool }
    editingRemoteAgentSetup.value = true
    options.currentPage.value = 'settings'
  }

  return {
    projectsScreen,
    requestProjectDetails,
    requestRunnerSetup,
    settingsScreen,
  }
}

export type AdminScreens = ReturnType<typeof useAdminScreens>
