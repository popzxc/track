import { computed, type ComputedRef, type Ref } from 'vue'

import { useAdminScreens } from './useAdminScreens'
import { useQueueScreens } from './useQueueScreens'
import type {
  MigrationImportSummary,
  MigrationStatus,
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  ReviewSummary,
  RunRecord,
  Task,
} from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface UseWorkflowScreensOptions {
  availableProjects: ComputedRef<ProjectInfo[]>
  canRequestReview: ComputedRef<boolean>
  currentPage: Ref<AppPage>
  defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
  errorMessage: Ref<string>
  migrationImportPending: Ref<boolean>
  migrationImportSummary: Ref<MigrationImportSummary | null>
  migrationStatus: Ref<MigrationStatus | null>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  reviewRequestDisabledReason: ComputedRef<string | undefined>
  runnerSetupReady: ComputedRef<boolean>
  saving: Ref<boolean>
  setFriendlyError: (error: unknown) => void
  shellPreludeHelpText: string
  tasks: Ref<Task[]>
  reviews: Ref<ReviewSummary[]>
  runs: Ref<RunRecord[]>
}

interface WorkflowDataLoaderBridge {
  loadRemoteAgentSettings: () => Promise<void>
  refreshAll: () => Promise<void>
}

/**
 * Owns the shell's screen graph so App.vue only composes top-level concerns.
 *
 * The shell has two kinds of coupling that are real and should stay explicit:
 * task/review selection feeds run-history loading, and settings can unblock a
 * queued dispatch that started from the task screen. Centralizing that wiring
 * here lets the rest of App.vue read as composition again instead of a long
 * sequence of refs, bridges, and hand-built screen controllers.
 */
export function useWorkflowScreens(options: UseWorkflowScreensOptions) {
  let requestProjectDetails: (project: ProjectInfo) => void = () => undefined
  let requestRunnerSetup: (task: Task, preferredTool: RemoteAgentPreferredTool) => void = () => undefined
  let refreshAllBridge: () => Promise<void> = async () => undefined

  const queueScreens = useQueueScreens({
    availableProjects: options.availableProjects,
    canRequestReview: options.canRequestReview,
    currentPage: options.currentPage,
    defaultRemoteAgentPreferredTool: options.defaultRemoteAgentPreferredTool,
    errorMessage: options.errorMessage,
    remoteAgentSettings: options.remoteAgentSettings,
    requestProjectDetails: (project: ProjectInfo) => requestProjectDetails(project),
    requestRunnerSetup: (task: Task, preferredTool: RemoteAgentPreferredTool) =>
      requestRunnerSetup(task, preferredTool),
    reviewRequestDisabledReason: options.reviewRequestDisabledReason,
    runnerSetupReady: options.runnerSetupReady,
    saving: options.saving,
    setFriendlyError: options.setFriendlyError,
    tasks: options.tasks,
    reviews: options.reviews,
    runs: options.runs,
  })

  const activeRemoteWorkCount = computed(
    () => queueScreens.activeRuns.value.length + queueScreens.activeReviewRuns.value.length,
  )

  const adminScreens = useAdminScreens({
    activeRemoteWorkCount,
    availableProjects: options.availableProjects,
    closeTaskDrawer: queueScreens.tasksScreen.closeTaskDrawer,
    currentPage: options.currentPage,
    errorMessage: options.errorMessage,
    migrationImportPending: options.migrationImportPending,
    migrationImportSummary: options.migrationImportSummary,
    migrationStatus: options.migrationStatus,
    remoteAgentSettings: options.remoteAgentSettings,
    refreshAll: async () => refreshAllBridge(),
    resumeQueuedTaskDispatch(task, preferredTool) {
      void queueScreens.resumeQueuedTaskDispatch(task, preferredTool)
    },
    runnerSetupReady: options.runnerSetupReady,
    saving: options.saving,
    selectedProjectFilter: queueScreens.tasksScreen.selectedProjectFilter,
    setFriendlyError: options.setFriendlyError,
    shellPreludeHelpText: options.shellPreludeHelpText,
  })

  requestProjectDetails = adminScreens.requestProjectDetails
  requestRunnerSetup = adminScreens.requestRunnerSetup

  function connectDataLoader(bridge: WorkflowDataLoaderBridge) {
    queueScreens.connectDataLoader(bridge)
    refreshAllBridge = bridge.refreshAll
  }

  return {
    activeRemoteWorkCount,
    activeReviewRuns: queueScreens.activeReviewRuns,
    activeRuns: queueScreens.activeRuns,
    connectDataLoader,
    importLegacyTrackerData: adminScreens.importLegacyTrackerData,
    loadLatestDispatchesForVisibleTasks: queueScreens.loadLatestDispatchesForVisibleTasks,
    loadReviews: queueScreens.loadReviews,
    loadRuns: queueScreens.loadRuns,
    loadSelectedReviewRunHistory: queueScreens.loadSelectedReviewRunHistory,
    loadSelectedTaskRunHistory: queueScreens.loadSelectedTaskRunHistory,
    projectsScreen: adminScreens.projectsScreen,
    resumeQueuedTaskDispatch: queueScreens.resumeQueuedTaskDispatch,
    reviewsScreen: queueScreens.reviewsScreen,
    runsScreen: queueScreens.runsScreen,
    settingsScreen: adminScreens.settingsScreen,
    tasksScreen: queueScreens.tasksScreen,
  }
}

export type WorkflowScreens = ReturnType<typeof useWorkflowScreens>
