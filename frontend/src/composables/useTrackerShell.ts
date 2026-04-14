import { computed, inject, onBeforeUnmount, onMounted, provide, ref, watch, type ComputedRef, type InjectionKey, type Ref } from 'vue'
import { useRoute } from 'vue-router'

import {
  ApiClientError,
  dispatchTask,
  fetchDispatches,
  fetchProjects,
  fetchRemoteAgentSettings,
  fetchReviews,
  fetchRuns,
  fetchTaskChangeVersion,
  fetchTaskRuns,
  fetchTasks,
  fetchReviewRuns,
} from '../api/client'
import { mergeProjects } from '../features/tasks/presentation'
import { firstQueryValue, queryFlag } from '../router/query'
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

const TASK_CHANGE_POLL_INTERVAL_MS = 2_000
const RUN_POLL_INTERVAL_MS = 60_000

interface TaskListFilter {
  project: string
  showClosed: boolean
}

interface TrackerShellContext {
  activeRemoteWorkCount: ComputedRef<number>
  activeReviewRuns: ComputedRef<ReviewSummary[]>
  activeRuns: ComputedRef<RunRecord[]>
  availableProjects: ComputedRef<ProjectInfo[]>
  canRequestReview: ComputedRef<boolean>
  defaultRemoteAgentPreferredTool: ComputedRef<RemoteAgentPreferredTool>
  errorMessage: Ref<string>
  latestTaskDispatchesByTaskId: Ref<Record<string, TaskDispatch>>
  loadRemoteAgentSettings: () => Promise<void>
  loadReviewRuns: (reviewId: string) => Promise<ReviewRunRecord[]>
  loadTaskRuns: (taskId: string) => Promise<RunRecord[]>
  loading: Ref<boolean>
  projects: Ref<ProjectInfo[]>
  recentReviewRuns: ComputedRef<ReviewSummary[]>
  recentRuns: ComputedRef<RunRecord[]>
  refreshAll: () => Promise<void>
  refreshing: Ref<boolean>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  reviewCount: ComputedRef<number>
  reviewRequestDisabledReason: ComputedRef<string | undefined>
  reviews: Ref<ReviewSummary[]>
  runnerSetupReady: ComputedRef<boolean>
  runs: Ref<RunRecord[]>
  saving: Ref<boolean>
  setFriendlyError: (error: unknown) => void
  shellPreludeHelpText: string
  startQueuedTaskDispatch: (task: Task, preferredTool: RemoteAgentPreferredTool) => Promise<void>
  tasks: Ref<Task[]>
  totalProjectCount: ComputedRef<number>
  upsertLatestReviewRun: (reviewId: string, latestRun: ReviewRunRecord) => void
  upsertLatestTaskDispatch: (dispatch: TaskDispatch) => void
  upsertReviewSummary: (review: ReviewRecord, latestRun?: ReviewRunRecord | null) => void
  upsertRunRecord: (task: Task, dispatch: TaskDispatch) => void
  upsertSelectedReviewRun: (
    selectedReviewId: Ref<string | null>,
    selectedReviewRuns: Ref<ReviewRunRecord[]>,
    run: ReviewRunRecord,
  ) => void
  upsertSelectedTaskRun: (
    selectedTaskId: Ref<string | null>,
    selectedTaskRuns: Ref<RunRecord[]>,
    task: Task,
    dispatch: TaskDispatch,
  ) => void
  removeReview: (
    selectedReviewId: Ref<string | null>,
    selectedReviewRuns: Ref<ReviewRunRecord[]>,
    reviewId: string,
  ) => void
  removeTaskRuns: (
    selectedTaskId: Ref<string | null>,
    selectedTaskRuns: Ref<RunRecord[]>,
    taskId: string,
  ) => void
  visibleTaskCount: ComputedRef<number>
}

const trackerShellContextKey: InjectionKey<TrackerShellContext> = Symbol('tracker-shell-context')

function reviewSummaryTimestamp(summary: ReviewSummary) {
  return summary.latestRun?.createdAt ?? summary.review.updatedAt ?? summary.review.createdAt
}

function sortReviewSummaries(reviewSummaries: ReviewSummary[]) {
  return reviewSummaries
    .slice()
    .sort((left, right) => Date.parse(reviewSummaryTimestamp(right)) - Date.parse(reviewSummaryTimestamp(left)))
}

function sortTaskRuns(runs: RunRecord[]) {
  return runs
    .slice()
    .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
}

function sortReviewRuns(runs: ReviewRunRecord[]) {
  return runs
    .slice()
    .sort((left, right) => Date.parse(right.createdAt) - Date.parse(left.createdAt))
}

function taskProjectOptions(tasks: Task[]): ProjectInfo[] {
  return tasks.map((task) => ({
    canonicalName: task.project,
    aliases: [],
    metadata: {
      repoUrl: '',
      gitUrl: '',
      baseBranch: 'main',
      description: undefined,
    },
  }))
}

export function provideTrackerShell(): TrackerShellContext {
  const route = useRoute()

  const taskFilter = ref<TaskListFilter>({
    project: '',
    showClosed: false,
  })

  const tasks = ref<Task[]>([])
  const reviews = ref<ReviewSummary[]>([])
  const projects = ref<ProjectInfo[]>([])
  const taskProjects = ref<ProjectInfo[]>([])
  const runs = ref<RunRecord[]>([])
  const latestTaskDispatchesByTaskId = ref<Record<string, TaskDispatch>>({})
  const remoteAgentSettings = ref<RemoteAgentSettings | null>(null)

  const loading = ref(true)
  const refreshing = ref(false)
  const saving = ref(false)
  const errorMessage = ref('')
  const taskChangeVersion = ref<number | null>(null)

  let taskChangePollTimer: number | null = null
  let taskChangePollInFlight = false
  let runPollTimer: number | null = null
  let runPollInFlight = false

  const visibleTaskCount = computed(() => tasks.value.length)
  const reviewCount = computed(() => reviews.value.length)
  const availableProjects = computed(() => mergeProjects(projects.value, taskProjects.value))
  const totalProjectCount = computed(() => availableProjects.value.length)
  const runnerSetupReady = computed(() =>
    Boolean(remoteAgentSettings.value?.configured && remoteAgentSettings.value.shellPrelude?.trim()),
  )
  const defaultRemoteAgentPreferredTool = computed<RemoteAgentPreferredTool>(
    () => remoteAgentSettings.value?.preferredTool ?? 'codex',
  )
  const reviewRequestDisabledReason = computed(() => {
    if (remoteAgentSettings.value && !remoteAgentSettings.value.configured) {
      return 'Remote dispatch is not configured yet. Run `track remote-agent configure --host <host> --user <user> --identity-file ~/.ssh/track_remote_agent` locally first.'
    }

    if (remoteAgentSettings.value && !runnerSetupReady.value) {
      return 'Save the runner shell prelude before requesting PR reviews.'
    }

    if (!remoteAgentSettings.value?.reviewFollowUp?.mainUser?.trim()) {
      return 'Set the main GitHub user in Settings to enable PR reviews.'
    }

    return undefined
  })
  const canRequestReview = computed(() => !reviewRequestDisabledReason.value)
  const activeRuns = computed(() =>
    runs.value
      .filter((run) => run.dispatch.status === 'preparing' || run.dispatch.status === 'running')
      .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt)),
  )
  const activeReviewRuns = computed(() =>
    reviews.value
      .filter((summary) => summary.latestRun?.status === 'preparing' || summary.latestRun?.status === 'running')
      .sort((left, right) => Date.parse(reviewSummaryTimestamp(right)) - Date.parse(reviewSummaryTimestamp(left))),
  )
  const recentRuns = computed(() =>
    runs.value
      .slice()
      .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
      .slice(0, 40),
  )
  const recentReviewRuns = computed(() =>
    reviews.value
      .filter((summary) => Boolean(summary.latestRun))
      .slice()
      .sort((left, right) => Date.parse(reviewSummaryTimestamp(right)) - Date.parse(reviewSummaryTimestamp(left)))
      .slice(0, 40),
  )
  const activeRemoteWorkCount = computed(
    () => activeRuns.value.length + activeReviewRuns.value.length,
  )
  const shellPreludeHelpText = 'The remote runner uses non-interactive SSH sessions, so it cannot rely on the environment tweaks that usually live in your interactive shell.\n\nKeep the shell prelude focused on PATH and toolchain setup. The backend reuses it before every remote command so dispatches stay predictable.'

  function setFriendlyError(error: unknown) {
    if (error instanceof ApiClientError) {
      errorMessage.value = error.message
      return
    }

    errorMessage.value =
      error instanceof Error ? error.message : 'Something went wrong while talking to the API.'
  }

  async function loadProjects() {
    projects.value = await fetchProjects()
  }

  async function loadRemoteAgentSettings() {
    remoteAgentSettings.value = await fetchRemoteAgentSettings()
  }

  async function loadTasks() {
    tasks.value = await fetchTasks({
      includeClosed: taskFilter.value.showClosed,
      project: taskFilter.value.project || undefined,
    })
    taskProjects.value = taskProjectOptions(tasks.value)
  }

  async function loadReviews() {
    reviews.value = sortReviewSummaries(await fetchReviews())
  }

  async function loadRuns() {
    runs.value = await fetchRuns(200)
  }

  async function loadLatestDispatchesForVisibleTasks() {
    const dispatches = await fetchDispatches(tasks.value.map((task) => task.id))

    const latestByTaskId: Record<string, TaskDispatch> = {}
    for (const dispatch of dispatches) {
      latestByTaskId[dispatch.taskId] = dispatch
    }

    latestTaskDispatchesByTaskId.value = latestByTaskId
  }

  async function loadTaskRuns(taskId: string) {
    return sortTaskRuns(await fetchTaskRuns(taskId))
  }

  async function loadReviewRuns(reviewId: string) {
    return sortReviewRuns(await fetchReviewRuns(reviewId))
  }

  async function refreshAll() {
    errorMessage.value = ''
    refreshing.value = true

    try {
      await Promise.all([
        loadProjects(),
        loadTasks(),
        loadReviews(),
        syncTaskChangeVersion(),
        loadRemoteAgentSettings().catch(() => {
          // Remote runner status is useful context, but the rest of the shell
          // should stay available if that endpoint is temporarily unavailable.
        }),
      ])

      loading.value = false

      await Promise.all([
        loadLatestDispatchesForVisibleTasks(),
        loadRuns(),
      ])
    } catch (error) {
      setFriendlyError(error)
    } finally {
      loading.value = false
      refreshing.value = false
    }
  }

  function upsertRunRecord(task: Task, dispatch: TaskDispatch) {
    const nextRecord: RunRecord = { task, dispatch }
    runs.value = sortTaskRuns([
      nextRecord,
      ...runs.value.filter((run) => run.dispatch.dispatchId !== dispatch.dispatchId),
    ])
  }

  function upsertLatestTaskDispatch(dispatch: TaskDispatch) {
    latestTaskDispatchesByTaskId.value = {
      ...latestTaskDispatchesByTaskId.value,
      [dispatch.taskId]: dispatch,
    }
  }

  function upsertSelectedTaskRun(
    selectedTaskId: Ref<string | null>,
    selectedTaskRuns: Ref<RunRecord[]>,
    task: Task,
    dispatch: TaskDispatch,
  ) {
    if (selectedTaskId.value !== task.id) {
      return
    }

    selectedTaskRuns.value = sortTaskRuns([
      { task, dispatch },
      ...selectedTaskRuns.value.filter((run) => run.dispatch.dispatchId !== dispatch.dispatchId),
    ])
  }

  function removeTaskRuns(
    selectedTaskId: Ref<string | null>,
    selectedTaskRuns: Ref<RunRecord[]>,
    taskId: string,
  ) {
    runs.value = runs.value.filter((run) => run.task.id !== taskId)
    const nextDispatches = { ...latestTaskDispatchesByTaskId.value }
    delete nextDispatches[taskId]
    latestTaskDispatchesByTaskId.value = nextDispatches

    if (selectedTaskId.value === taskId) {
      selectedTaskRuns.value = []
    }
  }

  function upsertReviewSummary(review: ReviewRecord, latestRun?: ReviewRunRecord | null) {
    const existingSummary = reviews.value.find((summary) => summary.review.id === review.id)
    reviews.value = sortReviewSummaries([
      {
        review,
        latestRun: latestRun ?? existingSummary?.latestRun,
      },
      ...reviews.value.filter((summary) => summary.review.id !== review.id),
    ])
  }

  function upsertLatestReviewRun(reviewId: string, latestRun: ReviewRunRecord) {
    reviews.value = sortReviewSummaries(
      reviews.value.map((summary) =>
        summary.review.id === reviewId
          ? { ...summary, latestRun }
          : summary),
    )
  }

  function upsertSelectedReviewRun(
    selectedReviewId: Ref<string | null>,
    selectedReviewRuns: Ref<ReviewRunRecord[]>,
    run: ReviewRunRecord,
  ) {
    if (selectedReviewId.value !== run.reviewId) {
      return
    }

    selectedReviewRuns.value = sortReviewRuns([
      run,
      ...selectedReviewRuns.value.filter((entry) => entry.dispatchId !== run.dispatchId),
    ])
  }

  function removeReview(
    selectedReviewId: Ref<string | null>,
    selectedReviewRuns: Ref<ReviewRunRecord[]>,
    reviewId: string,
  ) {
    reviews.value = reviews.value.filter((summary) => summary.review.id !== reviewId)

    if (selectedReviewId.value === reviewId) {
      selectedReviewRuns.value = []
    }
  }

  async function startQueuedTaskDispatch(task: Task, preferredTool: RemoteAgentPreferredTool) {
    const dispatch = await dispatchTask(task.id, { preferredTool })
    upsertRunRecord(task, dispatch)
    upsertLatestTaskDispatch(dispatch)
  }

  async function syncTaskChangeVersion() {
    taskChangeVersion.value = await fetchTaskChangeVersion()
  }

  async function pollForTaskChanges() {
    if (
      taskChangePollInFlight ||
      loading.value ||
      refreshing.value ||
      saving.value
    ) {
      return
    }

    taskChangePollInFlight = true

    try {
      const nextVersion = await fetchTaskChangeVersion()
      if (taskChangeVersion.value === null) {
        taskChangeVersion.value = nextVersion
        return
      }

      if (nextVersion !== taskChangeVersion.value) {
        await refreshAll()
        return
      }

      taskChangeVersion.value = nextVersion
    } catch {
      // Foreground actions already surface failures. Background sync should not
      // interrupt the shell when the filesystem watcher endpoint blips.
    } finally {
      taskChangePollInFlight = false
    }
  }

  async function pollForRunChanges() {
    if (
      runPollInFlight ||
      loading.value ||
      refreshing.value ||
      saving.value
    ) {
      return
    }

    if (activeRuns.value.length === 0 && activeReviewRuns.value.length === 0) {
      return
    }

    runPollInFlight = true

    try {
      await Promise.all([
        loadRuns(),
        loadReviews(),
        loadLatestDispatchesForVisibleTasks(),
      ])
    } catch {
      // The current run state remains useful even if one poll fails.
    } finally {
      runPollInFlight = false
    }
  }

  watch(
    () => [route.name ?? null, firstQueryValue(route.query.project), firstQueryValue(route.query.closed)] as const,
    ([pageName, project, closed]) => {
      if (pageName !== 'tasks') {
        return
      }

      taskFilter.value = {
        project: project ?? '',
        showClosed: queryFlag(closed),
      }
    },
    { immediate: true },
  )

  watch(taskFilter, () => {
    if (loading.value) {
      return
    }

    void (async () => {
      try {
        await loadTasks()
        await loadLatestDispatchesForVisibleTasks()
      } catch (error) {
        setFriendlyError(error)
      }
    })()
  }, { deep: true })

  onMounted(() => {
    void refreshAll()

    taskChangePollTimer = window.setInterval(() => {
      void pollForTaskChanges()
    }, TASK_CHANGE_POLL_INTERVAL_MS)

    runPollTimer = window.setInterval(() => {
      void pollForRunChanges()
    }, RUN_POLL_INTERVAL_MS)
  })

  onBeforeUnmount(() => {
    if (taskChangePollTimer !== null) {
      window.clearInterval(taskChangePollTimer)
    }

    if (runPollTimer !== null) {
      window.clearInterval(runPollTimer)
    }
  })

  const context: TrackerShellContext = {
    activeRemoteWorkCount,
    activeReviewRuns,
    activeRuns,
    availableProjects,
    canRequestReview,
    defaultRemoteAgentPreferredTool,
    errorMessage,
    latestTaskDispatchesByTaskId,
    loadRemoteAgentSettings,
    loadReviewRuns,
    loadTaskRuns,
    loading,
    projects,
    recentReviewRuns,
    recentRuns,
    refreshAll,
    refreshing,
    remoteAgentSettings,
    reviewCount,
    reviewRequestDisabledReason,
    reviews,
    runnerSetupReady,
    runs,
    saving,
    setFriendlyError,
    shellPreludeHelpText,
    startQueuedTaskDispatch,
    tasks,
    totalProjectCount,
    upsertLatestReviewRun,
    upsertLatestTaskDispatch,
    upsertReviewSummary,
    upsertRunRecord,
    upsertSelectedReviewRun,
    upsertSelectedTaskRun,
    removeReview,
    removeTaskRuns,
    visibleTaskCount,
  }

  provide(trackerShellContextKey, context)
  return context
}

export function useTrackerShell() {
  const context = inject(trackerShellContextKey)
  if (!context) {
    throw new Error('Tracker shell context is not available.')
  }

  return context
}
