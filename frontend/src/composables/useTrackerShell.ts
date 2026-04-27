import { computed, inject, onBeforeUnmount, onMounted, provide, ref, watch, type ComputedRef, type InjectionKey, type Ref } from 'vue'
import { useRoute } from 'vue-router'

import {
  ApiClientError,
  dispatchTask,
  fetchProjects,
  fetchRemoteAgentSettings,
  fetchTaskChangeVersion,
  fetchTasks,
} from '../api/client'
import { mergeProjects } from '../features/tasks/presentation'
import { firstQueryValue, queryFlag } from '../router/query'
import { useRunState } from './useRunState'
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
  removeReview: (reviewId: string) => void
  removeTaskRuns: (taskId: string) => void
  visibleTaskCount: ComputedRef<number>
}

const trackerShellContextKey: InjectionKey<TrackerShellContext> = Symbol('tracker-shell-context')

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
  const projects = ref<ProjectInfo[]>([])
  const taskProjects = ref<ProjectInfo[]>([])
  const runState = useRunState({ tasks })
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
  const reviewCount = computed(() => runState.reviews.value.length)
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
  const activeRemoteWorkCount = computed(
    () => runState.activeRuns.value.length + runState.activeReviewRuns.value.length,
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

  async function refreshAll() {
    errorMessage.value = ''
    refreshing.value = true

    try {
      await Promise.all([
        loadProjects(),
        loadTasks(),
        runState.loadReviews(),
        syncTaskChangeVersion(),
        loadRemoteAgentSettings().catch(() => {
          // Remote runner status is useful context, but the rest of the shell
          // should stay available if that endpoint is temporarily unavailable.
        }),
      ])

      loading.value = false

      await Promise.all([
        runState.loadLatestDispatchesForVisibleTasks(),
        runState.loadRuns(),
      ])
    } catch (error) {
      setFriendlyError(error)
    } finally {
      loading.value = false
      refreshing.value = false
    }
  }

  async function startQueuedTaskDispatch(task: Task, preferredTool: RemoteAgentPreferredTool) {
    const dispatch = await dispatchTask(task.id, { preferredTool })
    runState.upsertRunRecord(task, dispatch)
    runState.upsertLatestTaskDispatch(dispatch)
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

    if (runState.activeRuns.value.length === 0 && runState.activeReviewRuns.value.length === 0) {
      return
    }

    runPollInFlight = true

    try {
      await Promise.all([
        runState.loadRuns(),
        runState.loadReviews(),
        runState.loadLatestDispatchesForVisibleTasks(),
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
        await runState.loadLatestDispatchesForVisibleTasks()
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
    activeReviewRuns: runState.activeReviewRuns,
    activeRuns: runState.activeRuns,
    availableProjects,
    canRequestReview,
    defaultRemoteAgentPreferredTool,
    errorMessage,
    latestTaskDispatchesByTaskId: runState.latestTaskDispatchesByTaskId,
    loadRemoteAgentSettings,
    loadReviewRuns: runState.loadReviewRuns,
    loadTaskRuns: runState.loadTaskRuns,
    loading,
    projects,
    recentReviewRuns: runState.recentReviewRuns,
    recentRuns: runState.recentRuns,
    refreshAll,
    refreshing,
    remoteAgentSettings,
    reviewCount,
    reviewRequestDisabledReason,
    reviews: runState.reviews,
    runnerSetupReady,
    runs: runState.runs,
    saving,
    setFriendlyError,
    shellPreludeHelpText,
    startQueuedTaskDispatch,
    tasks,
    totalProjectCount,
    upsertLatestReviewRun: runState.upsertLatestReviewRun,
    upsertLatestTaskDispatch: runState.upsertLatestTaskDispatch,
    upsertReviewSummary: runState.upsertReviewSummary,
    upsertRunRecord: runState.upsertRunRecord,
    removeReview: runState.removeReview,
    removeTaskRuns: runState.removeTaskRuns,
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
