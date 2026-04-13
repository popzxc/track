import { dispatchTask } from '../api/client'
import { useReviewsScreenController } from './useReviewsScreenController'
import { useRunsScreenController } from './useRunsScreenController'
import { useRunState } from './useRunState'
import { useTasksScreenController } from './useTasksScreenController'
import type {
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'
import type { ComputedRef, Ref } from 'vue'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface QueueDataLoaderBridge {
  loadRemoteAgentSettings: () => Promise<void>
  refreshAll: () => Promise<void>
}

interface UseQueueScreensOptions {
  availableProjects: ComputedRef<ProjectInfo[]>
  canRequestReview: ComputedRef<boolean>
  currentPage: Ref<AppPage>
  defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
  errorMessage: Ref<string>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  requestProjectDetails: (project: ProjectInfo) => void
  requestRunnerSetup: (task: Task, preferredTool: RemoteAgentPreferredTool) => void
  reviewRequestDisabledReason: ComputedRef<string | undefined>
  runnerSetupReady: ComputedRef<boolean>
  saving: Ref<boolean>
  setFriendlyError: (error: unknown) => void
  tasks: Ref<Task[]>
  reviews: Ref<ReviewSummary[]>
  runs: Ref<RunRecord[]>
}

/**
 * Owns the queue-facing screens that share run-history state.
 *
 * Tasks, reviews, and runs are separate surfaces, but they all project the
 * same underlying dispatch history. This composable keeps that cycle together
 * so the shell and the admin surfaces do not have to understand how optimistic
 * run updates, drawer selections, and the Runs page stay synchronized.
 */
export function useQueueScreens(options: UseQueueScreensOptions) {
  let loadRemoteAgentSettingsBridge: () => Promise<void> = async () => undefined
  let refreshAllBridge: () => Promise<void> = async () => undefined
  let loadRunsBridge: () => Promise<void> = async () => undefined
  let removeTaskRunsBridge: (taskId: string) => void = () => undefined
  let upsertLatestTaskDispatchBridge: (dispatch: TaskDispatch) => void = () => undefined
  let upsertRunRecordBridge: (task: Task, dispatch: TaskDispatch) => void = () => undefined
  let upsertSelectedTaskRunBridge: (task: Task, dispatch: TaskDispatch) => void = () => undefined
  let removeReviewBridge: (reviewId: string) => void = () => undefined
  let replaceSelectedReviewRunsBridge: (reviewRuns: ReviewRunRecord[]) => void = () => undefined
  let upsertLatestReviewRunBridge: (reviewId: string, latestRun: ReviewRunRecord) => void = () => undefined
  let upsertReviewSummaryBridge: (review: ReviewRecord, latestRun?: ReviewRunRecord | null) => void = () => undefined
  let upsertSelectedReviewRunBridge: (run: ReviewRunRecord) => void = () => undefined

  const tasksScreen = useTasksScreenController({
    data: {
      availableProjects: options.availableProjects,
      defaultRemoteAgentPreferredTool: options.defaultRemoteAgentPreferredTool,
      remoteAgentSettings: options.remoteAgentSettings,
      runnerSetupReady: options.runnerSetupReady,
      saving: options.saving,
      tasks: options.tasks,
    },
    project: {
      selectProjectDetails: options.requestProjectDetails,
    },
    settings: {
      requestRunnerSetup: options.requestRunnerSetup,
    },
    shell: {
      currentPage: options.currentPage,
      errorMessage: options.errorMessage,
      setFriendlyError: options.setFriendlyError,
    },
    taskRunBridge: {
      loadRemoteAgentSettings: async () => loadRemoteAgentSettingsBridge(),
      loadRuns: async () => loadRunsBridge(),
      refreshAll: async () => refreshAllBridge(),
      removeTaskRuns: (taskId: string) => removeTaskRunsBridge(taskId),
      upsertLatestTaskDispatch: (dispatch: TaskDispatch) => upsertLatestTaskDispatchBridge(dispatch),
      upsertRunRecord: (task: Task, dispatch: TaskDispatch) => upsertRunRecordBridge(task, dispatch),
      upsertSelectedTaskRun: (task: Task, dispatch: TaskDispatch) => upsertSelectedTaskRunBridge(task, dispatch),
    },
  })

  const reviewsScreen = useReviewsScreenController({
    data: {
      canRequestReview: options.canRequestReview,
      defaultRemoteAgentPreferredTool: options.defaultRemoteAgentPreferredTool,
      remoteAgentSettings: options.remoteAgentSettings,
      reviewRequestDisabledReason: options.reviewRequestDisabledReason,
      reviews: options.reviews,
      saving: options.saving,
    },
    reviewRunBridge: {
      refreshAll: async () => refreshAllBridge(),
      removeReview: (reviewId: string) => removeReviewBridge(reviewId),
      replaceSelectedReviewRuns: (reviewRuns: ReviewRunRecord[]) => replaceSelectedReviewRunsBridge(reviewRuns),
      upsertLatestReviewRun: (reviewId: string, latestRun: ReviewRunRecord) => upsertLatestReviewRunBridge(reviewId, latestRun),
      upsertReviewSummary: (review: ReviewRecord, latestRun?: ReviewRunRecord | null) => upsertReviewSummaryBridge(review, latestRun),
      upsertSelectedReviewRun: (run: ReviewRunRecord) => upsertSelectedReviewRunBridge(run),
    },
    shell: {
      currentPage: options.currentPage,
      errorMessage: options.errorMessage,
      setFriendlyError: options.setFriendlyError,
    },
  })

  const runState = useRunState({
    closeReviewDrawer: reviewsScreen.closeReviewDrawer,
    isReviewDrawerOpen: reviewsScreen.isReviewDrawerOpen,
    isTaskDrawerOpen: tasksScreen.isTaskDrawerOpen,
    latestTaskDispatchesByTaskId: tasksScreen.latestTaskDispatchesByTaskId,
    reviews: options.reviews,
    runs: options.runs,
    selectedReview: reviewsScreen.selectedReview,
    selectedReviewId: reviewsScreen.selectedReviewId,
    selectedReviewRuns: reviewsScreen.selectedReviewRuns,
    selectedTask: tasksScreen.selectedTask,
    selectedTaskId: tasksScreen.selectedTaskId,
    selectedTaskRuns: tasksScreen.selectedTaskRuns,
    tasks: options.tasks,
  })

  loadRunsBridge = runState.loadRuns
  removeTaskRunsBridge = runState.removeTaskRuns
  upsertLatestTaskDispatchBridge = runState.upsertLatestTaskDispatch
  upsertRunRecordBridge = runState.upsertRunRecord
  upsertSelectedTaskRunBridge = runState.upsertSelectedTaskRun
  removeReviewBridge = runState.removeReview
  replaceSelectedReviewRunsBridge = runState.replaceSelectedReviewRuns
  upsertLatestReviewRunBridge = runState.upsertLatestReviewRun
  upsertReviewSummaryBridge = runState.upsertReviewSummary
  upsertSelectedReviewRunBridge = runState.upsertSelectedReviewRun

  const runsScreen = useRunsScreenController({
    activeReviewRuns: runState.activeReviewRuns,
    activeRuns: runState.activeRuns,
    openTaskFromRun: tasksScreen.openTaskFromRun,
    recentReviewRuns: runState.recentReviewRuns,
    recentRuns: runState.recentRuns,
    selectReview: reviewsScreen.selectReview,
  })

  function connectDataLoader(bridge: QueueDataLoaderBridge) {
    loadRemoteAgentSettingsBridge = bridge.loadRemoteAgentSettings
    refreshAllBridge = bridge.refreshAll
  }

  async function resumeQueuedTaskDispatch(task: Task, preferredTool: RemoteAgentPreferredTool) {
    tasksScreen.dispatchingTaskId.value = task.id
    options.errorMessage.value = ''

    try {
      const dispatch = await dispatchTask(task.id, { preferredTool })
      runState.upsertRunRecord(task, dispatch)
      runState.upsertLatestTaskDispatch(dispatch)
      runState.upsertSelectedTaskRun(task, dispatch)
    } catch (error) {
      await runState.loadRuns().catch(() => undefined)
      options.setFriendlyError(error)
    } finally {
      tasksScreen.dispatchingTaskId.value = null
    }
  }

  return {
    activeReviewRuns: runState.activeReviewRuns,
    activeRuns: runState.activeRuns,
    connectDataLoader,
    loadLatestDispatchesForVisibleTasks: runState.loadLatestDispatchesForVisibleTasks,
    loadReviews: runState.loadReviews,
    loadRuns: runState.loadRuns,
    loadSelectedReviewRunHistory: runState.loadSelectedReviewRunHistory,
    loadSelectedTaskRunHistory: runState.loadSelectedTaskRunHistory,
    resumeQueuedTaskDispatch,
    reviewsScreen,
    runsScreen,
    tasksScreen,
  }
}

export type QueueScreens = ReturnType<typeof useQueueScreens>
