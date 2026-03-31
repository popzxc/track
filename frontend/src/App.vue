<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import {
  ApiClientError,
  cancelDispatch,
  cancelReview,
  cleanupRemoteAgentArtifacts,
  createReview,
  createTask,
  deleteReview,
  deleteTask,
  discardDispatch,
  dispatchTask,
  fetchDispatches,
  fetchMigrationStatus,
  fetchProjects,
  followUpReview,
  fetchReviewRuns,
  fetchReviews,
  fetchRemoteAgentSettings,
  fetchRuns,
  fetchTaskRuns,
  fetchTaskChangeVersion,
  fetchTasks,
  followUpTask,
  importLegacyData,
  resetRemoteAgentWorkspace,
  updateProject,
  updateRemoteAgentSettings,
  updateTask,
} from './api/client'
import ConfirmDialog from './components/ConfirmDialog.vue'
import FollowUpModal from './components/FollowUpModal.vue'
import ProjectMetadataModal from './components/ProjectMetadataModal.vue'
import ReviewFollowUpModal from './components/ReviewFollowUpModal.vue'
import ReviewRequestModal from './components/ReviewRequestModal.vue'
import RemoteAgentSetupModal from './components/RemoteAgentSetupModal.vue'
import TaskEditorModal from './components/TaskEditorModal.vue'
import {
  dispatchBadgeClass,
  dispatchStatusLabel,
  dispatchSummary,
  drawerPrimaryAction,
  formatDateTime,
  formatTaskTimestamp,
  getRunStartDisabledReason,
  groupTasksByProject,
  mergeProjects,
  priorityBadgeClass,
  taskReference,
  taskStatusBadgeClass,
} from './features/tasks/presentation'
import {
  parseTaskDescription,
  taskTitle,
  type ParsedTaskDescription,
} from './features/tasks/description'
import type {
  CreateReviewInput,
  MigrationImportSummary,
  MigrationStatus,
  ProjectInfo,
  ProjectMetadataUpdateInput,
  RemoteCleanupSummary,
  RemoteResetSummary,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
  ReviewRecord,
  ReviewFollowUpInput,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
  TaskCreateInput,
  TaskDispatch,
  TaskFollowUpInput,
} from './types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'
type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'
type PendingRunnerSetupRequest = {
  task: Task
  preferredTool: RemoteAgentPreferredTool
}

const TASK_CHANGE_POLL_INTERVAL_MS = 2_000

// Remote Codex runs are deliberately refreshed more slowly than local task
// files. New tasks should appear almost immediately, while long-running remote
// work should not turn into constant SSH-backed churn.
const RUN_POLL_INTERVAL_MS = 60_000

// =============================================================================
// App Shell State
// =============================================================================
//
// The app now treats the queue as the primary surface and task details as a
// transient drawer. We still keep the state in one shell component because the
// flows are closely related and the project remains small enough to avoid a
// global store or router.
// TODO: Promote this shell into route-backed screens if any page starts
// needing deep links or significantly more local state.
const currentPage = ref<AppPage>('tasks')
const tasks = ref<Task[]>([])
const reviews = ref<ReviewSummary[]>([])
const projects = ref<ProjectInfo[]>([])
const taskProjectOptions = ref<ProjectInfo[]>([])
const runs = ref<RunRecord[]>([])
const latestTaskDispatchesByTaskId = ref<Record<string, TaskDispatch>>({})
const selectedTaskRuns = ref<RunRecord[]>([])
const selectedReviewRuns = ref<ReviewRunRecord[]>([])
const remoteAgentSettings = ref<RemoteAgentSettings | null>(null)
const selectedTaskStartTool = ref<RemoteAgentPreferredTool>('codex')
const showClosed = ref(false)
const selectedProjectFilter = ref('')
const selectedTaskId = ref<string | null>(null)
const selectedReviewId = ref<string | null>(null)
const pendingSelectedTaskId = ref<string | null>(null)
const selectedProjectDetailsId = ref<string | null>(null)
const isTaskDrawerOpen = ref(false)
const isReviewDrawerOpen = ref(false)
const taskChangeVersion = ref<number | null>(null)
const loading = ref(true)
const refreshing = ref(false)
const saving = ref(false)
const dispatchingTaskId = ref<string | null>(null)
const cancelingDispatchTaskId = ref<string | null>(null)
const cancelingReviewId = ref<string | null>(null)
const discardingDispatchTaskId = ref<string | null>(null)
const followingUpTaskId = ref<string | null>(null)
const followingUpReviewId = ref<string | null>(null)
const taskLifecycleMutationTaskId = ref<string | null>(null)
const taskLifecycleMutation = ref<TaskLifecycleMutation | null>(null)
const errorMessage = ref('')

const creatingTask = ref(false)
const creatingReview = ref(false)
const editingTask = ref<Task | null>(null)
const editingProject = ref<ProjectInfo | null>(null)
const editingRemoteAgentSetup = ref(false)
const followingUpTask = ref<Task | null>(null)
const followingUpReview = ref<ReviewRecord | null>(null)
const taskPendingDeletion = ref<Task | null>(null)
const reviewPendingDeletion = ref<ReviewRecord | null>(null)
const taskPendingRunnerSetup = ref<PendingRunnerSetupRequest | null>(null)
const cleanupPendingConfirmation = ref(false)
const cleaningUpRemoteArtifacts = ref(false)
const cleanupSummary = ref<RemoteCleanupSummary | null>(null)
const resetPendingConfirmation = ref(false)
const resettingRemoteWorkspace = ref(false)
const resetSummary = ref<RemoteResetSummary | null>(null)
const migrationStatus = ref<MigrationStatus | null>(null)
const migrationImportSummary = ref<MigrationImportSummary | null>(null)
const migrationImportPending = ref(false)

let taskChangePollTimer: number | null = null
let taskChangePollInFlight = false
let runPollTimer: number | null = null
let runPollInFlight = false
let selectedTaskRunsRequestVersion = 0
let selectedReviewRunsRequestVersion = 0

// =============================================================================
// Derived State
// =============================================================================
//
// The redesign keeps "tasks", "runs", and "project metadata" as separate
// concepts. The queue stays quiet, while richer context lives in the drawer and
// the dedicated Runs / Projects pages.
const visibleTaskCount = computed(() => tasks.value.length)
const reviewCount = computed(() => reviews.value.length)
const totalProjectCount = computed(() => availableProjects.value.length)
const runnerSetupReady = computed(() =>
  Boolean(remoteAgentSettings.value?.configured && remoteAgentSettings.value.shellPrelude?.trim()),
)
const defaultRemoteAgentPreferredTool = computed<RemoteAgentPreferredTool>(
  () => remoteAgentSettings.value?.preferredTool ?? 'codex',
)

const availableProjects = computed(() => mergeProjects(projects.value, taskProjectOptions.value))
const reviewRequestDisabledReason = computed(() => {
  if (remoteAgentSettings.value && !remoteAgentSettings.value.configured) {
    return 'Remote dispatch is not configured yet. Run `track remote-agent configure --host <host> --user <user> --identity-file ~/.ssh/id_ed25519` locally first.'
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
const migrationRequired = computed(() => Boolean(migrationStatus.value?.requiresMigration))

// =============================================================================
// Task Grouping
// =============================================================================
//
// "All projects" becomes hard to scan once the queue grows. Instead of one long
// mixed stream, the queue is grouped into project sections while keeping the
// existing per-task sort order inside each section. This preserves the backend's
// task ordering semantics without forcing the user to mentally re-cluster rows.
const taskGroups = computed(() => {
  return groupTasksByProject(tasks.value)
})

const latestDispatchByTaskId = computed<Record<string, TaskDispatch>>(
  () => latestTaskDispatchesByTaskId.value,
)

const selectedTask = computed(() =>
  tasks.value.find((task) => task.id === selectedTaskId.value) ?? null,
)

const selectedTaskProject = computed(() =>
  selectedTask.value
    ? availableProjects.value.find((project) => project.canonicalName === selectedTask.value?.project) ?? null
    : null,
)

const selectedTaskLatestDispatch = computed(() =>
  selectedTask.value ? latestDispatchByTaskId.value[selectedTask.value.id] ?? null : null,
)

const selectedTaskPinnedTool = computed<RemoteAgentPreferredTool | null>(
  () => selectedTaskLatestDispatch.value?.preferredTool ?? null,
)

const selectedTaskDispatchTool = computed<RemoteAgentPreferredTool>(
  () => selectedTaskPinnedTool.value ?? selectedTaskStartTool.value,
)

const selectedTaskDescription = computed<ParsedTaskDescription | null>(() =>
  selectedTask.value ? parseTaskDescription(selectedTask.value.description) : null,
)

const selectedTaskLatestReusablePullRequest = computed(() =>
  selectedTaskRuns.value.find((run) => Boolean(run.dispatch.pullRequestUrl))?.dispatch.pullRequestUrl
    ?? selectedTaskLatestDispatch.value?.pullRequestUrl
    ?? null,
)

const selectedTaskLifecycleMutation = computed(() =>
  selectedTask.value && taskLifecycleMutationTaskId.value === selectedTask.value.id
    ? taskLifecycleMutation.value
    : null,
)

const selectedTaskDispatchDisabledReason = computed(() =>
  selectedTask.value
    ? getRunStartDisabledReason(selectedTask.value, availableProjects.value, remoteAgentSettings.value)
    : undefined,
)

const selectedTaskCanContinue = computed(() =>
  Boolean(
    selectedTask.value &&
      selectedTaskLatestDispatch.value &&
      !selectedTaskDispatchDisabledReason.value &&
      selectedTaskLatestDispatch.value.status !== 'preparing' &&
      selectedTaskLatestDispatch.value.status !== 'running' &&
      selectedTaskLatestDispatch.value.branchName &&
      selectedTaskLatestDispatch.value.worktreePath,
  ),
)

const selectedTaskCanStartFresh = computed(() =>
  Boolean(
    selectedTask.value &&
      selectedTask.value.status === 'open' &&
      !selectedTaskDispatchDisabledReason.value &&
      selectedTaskLatestDispatch.value &&
      selectedTaskLatestDispatch.value.status !== 'preparing' &&
      selectedTaskLatestDispatch.value.status !== 'running',
  ),
)

const selectedTaskCanDiscardHistory = computed(() =>
  Boolean(
    selectedTask.value &&
      selectedTaskLatestDispatch.value &&
      selectedTaskLatestDispatch.value.status !== 'preparing' &&
      selectedTaskLatestDispatch.value.status !== 'running',
  ),
)

const selectedReviewSummary = computed(() =>
  reviews.value.find((summary) => summary.review.id === selectedReviewId.value) ?? null,
)

const selectedReview = computed(() => selectedReviewSummary.value?.review ?? null)

const selectedReviewLatestRun = computed(() => selectedReviewSummary.value?.latestRun ?? null)

const selectedReviewCanCancel = computed(() =>
  Boolean(
    selectedReview.value &&
      selectedReviewLatestRun.value &&
      (selectedReviewLatestRun.value.status === 'preparing' || selectedReviewLatestRun.value.status === 'running'),
  ),
)

const selectedReviewCanReReview = computed(() =>
  Boolean(selectedReview.value && !selectedReviewCanCancel.value),
)

const activeRuns = computed(() =>
  runs.value
    .filter((run) => run.dispatch.status === 'preparing' || run.dispatch.status === 'running')
    .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt)),
)

const activeReviewRuns = computed(() =>
  reviews.value
    .filter(
      (summary) =>
        summary.latestRun?.status === 'preparing' || summary.latestRun?.status === 'running',
    )
    .sort((left, right) => {
      const leftCreatedAt = left.latestRun?.createdAt ?? left.review.updatedAt ?? left.review.createdAt
      const rightCreatedAt = right.latestRun?.createdAt ?? right.review.updatedAt ?? right.review.createdAt
      return Date.parse(rightCreatedAt) - Date.parse(leftCreatedAt)
    }),
)

const activeRemoteWorkCount = computed(() => activeRuns.value.length + activeReviewRuns.value.length)

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
    .sort((left, right) => {
      const leftCreatedAt = left.latestRun?.createdAt ?? left.review.updatedAt ?? left.review.createdAt
      const rightCreatedAt = right.latestRun?.createdAt ?? right.review.updatedAt ?? right.review.createdAt
      return Date.parse(rightCreatedAt) - Date.parse(leftCreatedAt)
    })
    .slice(0, 40),
)

const selectedProjectRecord = computed(() =>
  availableProjects.value.find((project) => project.canonicalName === selectedProjectFilter.value) ?? null,
)

const selectedProjectDetails = computed(() =>
  availableProjects.value.find((project) => project.canonicalName === selectedProjectDetailsId.value) ?? null,
)

const defaultCreateProject = computed(
  () =>
    selectedProjectRecord.value?.canonicalName ??
    availableProjects.value[0]?.canonicalName ??
    '',
)

const followingUpDispatch = computed(() =>
  followingUpTask.value ? latestDispatchByTaskId.value[followingUpTask.value.id] ?? undefined : undefined,
)

const shellPreludeHelpText = 'The remote runner uses non-interactive SSH sessions, so it cannot rely on the environment tweaks that usually live in your interactive shell.\n\nKeep the shell prelude focused on PATH and toolchain setup. The backend reuses it before every remote command so dispatches stay predictable.'

function remoteAgentToolLabel(tool: RemoteAgentPreferredTool | null | undefined): string {
  return tool === 'claude' ? 'Claude' : 'Codex'
}

// =============================================================================
// Presentation Helpers
// =============================================================================
//
// The UI intentionally keeps the queue dense and reserves stronger styling for
// actual run outcomes. Priority remains visible, but it no longer competes with
// failure states for the loudest color on screen.
function drawerPrimaryActionLabel(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return selectedTaskLifecycleMutation.value === 'reopening' ? 'Reopening...' : 'Reopen task'
    case 'cancel':
      return cancelingDispatchTaskId.value === task.id ? 'Canceling...' : 'Cancel run'
    case 'continue':
      return followingUpTaskId.value === task.id ? 'Continuing...' : 'Continue run'
    case 'start':
      return dispatchingTaskId.value === task.id ? 'Starting...' : 'Start agent'
  }
}

function drawerPrimaryActionClass(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return 'border border-yellow/30 bg-yellow/10 text-yellow hover:bg-yellow/15'
    case 'cancel':
      return 'border border-orange/30 bg-orange/10 text-orange hover:bg-orange/15'
    case 'continue':
      return 'border border-aqua/30 bg-aqua/10 text-aqua hover:bg-aqua/15'
    case 'start':
      return 'border border-blue/30 bg-blue/10 text-blue hover:bg-blue/15'
  }
}

function beginTaskLifecycleMutation(taskId: string, mutation: TaskLifecycleMutation) {
  taskLifecycleMutationTaskId.value = taskId
  taskLifecycleMutation.value = mutation
}

function clearTaskLifecycleMutation() {
  taskLifecycleMutationTaskId.value = null
  taskLifecycleMutation.value = null
}

function taskLifecycleProgressMessage(mutation: TaskLifecycleMutation | null): string {
  switch (mutation) {
    case 'closing':
      return 'Closing the task and cleaning up its remote worktree...'
    case 'reopening':
      return 'Reopening the task so you can continue work...'
    case 'deleting':
      return 'Deleting the task and removing its remote artifacts...'
    case null:
      return ''
  }
}

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}

function migrationCleanupCommand(path: string) {
  return path.endsWith('.json') ? `rm -f ${path}` : `rm -rf ${path}`
}

function setFriendlyError(error: unknown) {
  if (error instanceof ApiClientError) {
    errorMessage.value = error.message
    return
  }

  errorMessage.value =
    error instanceof Error ? error.message : 'Something went wrong while talking to the API.'
}

function upsertRunRecord(task: Task, dispatch: TaskDispatch) {
  const nextRecord: RunRecord = { task, dispatch }
  runs.value = [nextRecord, ...runs.value.filter((run) => run.dispatch.dispatchId !== dispatch.dispatchId)]
    .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
}

function upsertLatestTaskDispatch(dispatch: TaskDispatch) {
  latestTaskDispatchesByTaskId.value = {
    ...latestTaskDispatchesByTaskId.value,
    [dispatch.taskId]: dispatch,
  }
}

function replaceSelectedTaskRuns(taskRuns: RunRecord[]) {
  selectedTaskRuns.value = taskRuns
    .slice()
    .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
}

function upsertSelectedTaskRun(task: Task, dispatch: TaskDispatch) {
  if (selectedTaskId.value !== task.id) {
    return
  }

  replaceSelectedTaskRuns([
    { task, dispatch },
    ...selectedTaskRuns.value.filter((run) => run.dispatch.dispatchId !== dispatch.dispatchId),
  ])
}

function removeTaskRuns(taskId: string) {
  runs.value = runs.value.filter((run) => run.task.id !== taskId)
  const nextDispatches = { ...latestTaskDispatchesByTaskId.value }
  delete nextDispatches[taskId]
  latestTaskDispatchesByTaskId.value = nextDispatches

  if (selectedTaskId.value === taskId) {
    selectedTaskRuns.value = []
  }
}

function reviewSummaryTimestamp(summary: ReviewSummary) {
  return summary.latestRun?.createdAt ?? summary.review.updatedAt ?? summary.review.createdAt
}

function sortReviewSummaries(reviewSummaries: ReviewSummary[]) {
  return reviewSummaries
    .slice()
    .sort((left, right) => Date.parse(reviewSummaryTimestamp(right)) - Date.parse(reviewSummaryTimestamp(left)))
}

function replaceSelectedReviewRuns(reviewRuns: ReviewRunRecord[]) {
  selectedReviewRuns.value = reviewRuns
    .slice()
    .sort((left, right) => Date.parse(right.createdAt) - Date.parse(left.createdAt))
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

function upsertSelectedReviewRun(run: ReviewRunRecord) {
  if (selectedReviewId.value !== run.reviewId) {
    return
  }

  replaceSelectedReviewRuns([
    run,
    ...selectedReviewRuns.value.filter((entry) => entry.dispatchId !== run.dispatchId),
  ])
}

function removeReview(reviewId: string) {
  reviews.value = reviews.value.filter((summary) => summary.review.id !== reviewId)

  if (selectedReviewId.value === reviewId) {
    closeReviewDrawer()
  }
}

function selectTask(taskId: string) {
  selectedTaskId.value = taskId
  isTaskDrawerOpen.value = true

  if (currentPage.value !== 'tasks') {
    currentPage.value = 'tasks'
  }
}

function closeTaskDrawer() {
  isTaskDrawerOpen.value = false
  selectedTaskId.value = null
}

function selectReview(reviewId: string) {
  selectedReviewId.value = reviewId
  isReviewDrawerOpen.value = true

  if (currentPage.value !== 'reviews') {
    currentPage.value = 'reviews'
  }
}

function closeReviewDrawer() {
  isReviewDrawerOpen.value = false
  selectedReviewId.value = null
  selectedReviewRuns.value = []
  followingUpReview.value = null
}

function openTaskFromRun(run: RunRecord) {
  currentPage.value = 'tasks'
  pendingSelectedTaskId.value = run.task.id
  isTaskDrawerOpen.value = true

  const needsProjectFilterChange = selectedProjectFilter.value !== run.task.project
  const needsClosedTasks = run.task.status === 'closed' && !showClosed.value

  selectedProjectFilter.value = run.task.project
  if (run.task.status === 'closed') {
    showClosed.value = true
  }

  if (!needsProjectFilterChange && !needsClosedTasks) {
    selectedTaskId.value = run.task.id
    pendingSelectedTaskId.value = null
  }
}

function selectProjectDetails(project: ProjectInfo) {
  selectedProjectDetailsId.value = project.canonicalName
  currentPage.value = 'projects'
  isTaskDrawerOpen.value = false
}

// =============================================================================
// Data Loading
// =============================================================================
//
// Each loader owns one slice of backend truth. Foreground mutations still
// refresh from the server because the filesystem and persisted run history are
// the real source of truth.
async function loadProjects() {
  projects.value = await fetchProjects()
}

async function loadMigrationGate() {
  migrationStatus.value = await fetchMigrationStatus()
}

async function loadRemoteAgentSettings() {
  remoteAgentSettings.value = await fetchRemoteAgentSettings()
}

async function loadTasks() {
  tasks.value = await fetchTasks({
    includeClosed: showClosed.value,
    project: selectedProjectFilter.value || undefined,
  })

  taskProjectOptions.value = tasks.value.map((task) => ({
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

async function loadReviews() {
  reviews.value = sortReviewSummaries(await fetchReviews())
}

async function loadLatestDispatchesForVisibleTasks() {
  const dispatches = await fetchDispatches(tasks.value.map((task) => task.id))

  const latestByTaskId: Record<string, TaskDispatch> = {}
  for (const dispatch of dispatches) {
    latestByTaskId[dispatch.taskId] = dispatch
  }

  latestTaskDispatchesByTaskId.value = latestByTaskId
}

async function loadSelectedTaskRunHistory() {
  if (!isTaskDrawerOpen.value || !selectedTask.value) {
    selectedTaskRuns.value = []
    return
  }

  const requestVersion = ++selectedTaskRunsRequestVersion
  const taskId = selectedTask.value.id
  const taskRuns = await fetchTaskRuns(taskId)

  // The drawer is transient, so stale responses should not overwrite a newer
  // selection if the user switched tasks while the request was in flight.
  if (
    requestVersion !== selectedTaskRunsRequestVersion
    || !isTaskDrawerOpen.value
    || selectedTask.value?.id !== taskId
  ) {
    return
  }

  replaceSelectedTaskRuns(taskRuns)
}

async function loadSelectedReviewRunHistory() {
  if (!isReviewDrawerOpen.value || !selectedReview.value) {
    selectedReviewRuns.value = []
    return
  }

  const requestVersion = ++selectedReviewRunsRequestVersion
  const reviewId = selectedReview.value.id
  const reviewRuns = await fetchReviewRuns(reviewId)

  // The review drawer is also transient, so we apply the same stale-response
  // protection that task run history already uses.
  if (
    requestVersion !== selectedReviewRunsRequestVersion
    || !isReviewDrawerOpen.value
    || selectedReview.value?.id !== reviewId
  ) {
    return
  }

  replaceSelectedReviewRuns(reviewRuns)
}

async function loadRuns() {
  runs.value = await fetchRuns(200)
}

async function syncTaskChangeVersion() {
  taskChangeVersion.value = await fetchTaskChangeVersion()
}

function resetAppDataForMigration() {
  tasks.value = []
  reviews.value = []
  projects.value = []
  taskProjectOptions.value = []
  runs.value = []
  latestTaskDispatchesByTaskId.value = {}
  selectedTaskRuns.value = []
  selectedReviewRuns.value = []
  remoteAgentSettings.value = null
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
        // Runner setup is useful context, but the rest of the app should still
        // render if that endpoint is temporarily unavailable.
      }),
    ])

    await Promise.all([
      loadLatestDispatchesForVisibleTasks(),
      loadRuns(),
      loadSelectedTaskRunHistory().catch(() => {
        // The drawer can still show the task body if task-scoped run history
        // is temporarily unavailable.
      }),
      loadSelectedReviewRunHistory().catch(() => {
        // The review drawer can still show the persisted review record if its
        // run history is temporarily unavailable.
      }),
    ])
    migrationStatus.value = null
  } catch (error) {
    if (error instanceof ApiClientError && error.code === 'MIGRATION_REQUIRED') {
      try {
        await loadMigrationGate()
        resetAppDataForMigration()
      } catch (migrationError) {
        setFriendlyError(migrationError)
      }
    } else {
      setFriendlyError(error)
    }
  } finally {
    loading.value = false
    refreshing.value = false
  }
}

// =============================================================================
// Mutations
// =============================================================================
//
// The app stays deliberately conservative after writes: refresh from the API,
// then let the queue, drawer, and runs page re-derive their state. That keeps
// the UI aligned with the persisted task and dispatch data.
async function updateTaskStatus(task: Task, status: Task['status']) {
  saving.value = true
  errorMessage.value = ''
  beginTaskLifecycleMutation(task.id, status === 'closed' ? 'closing' : 'reopening')

  try {
    await updateTask(task.id, { status })
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
    clearTaskLifecycleMutation()
  }
}

async function saveTaskEdits(payload: { description: string; priority: Task['priority'] }) {
  if (!editingTask.value) {
    return
  }

  saving.value = true
  errorMessage.value = ''

  try {
    await updateTask(editingTask.value.id, payload)
    editingTask.value = null
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function createTaskFromWeb(payload: TaskCreateInput) {
  saving.value = true
  errorMessage.value = ''

  try {
    const task = await createTask(payload)
    creatingTask.value = false
    pendingSelectedTaskId.value = task.id
    isTaskDrawerOpen.value = true
    currentPage.value = 'tasks'
    selectedProjectFilter.value = task.project
    showClosed.value = false
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function createReviewFromWeb(payload: CreateReviewInput) {
  saving.value = true
  errorMessage.value = ''

  try {
    const created = await createReview(payload)
    creatingReview.value = false
    currentPage.value = 'reviews'
    selectReview(created.review.id)
    upsertReviewSummary(created.review, created.run)
    replaceSelectedReviewRuns([created.run])
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function saveProjectEdits(payload: ProjectMetadataUpdateInput) {
  if (!editingProject.value) {
    return
  }

  saving.value = true
  errorMessage.value = ''

  try {
    await updateProject(editingProject.value.canonicalName, payload)
    editingProject.value = null
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function saveRemoteAgentSetup(payload: RemoteAgentSettingsUpdateInput) {
  saving.value = true
  errorMessage.value = ''

  try {
    remoteAgentSettings.value = await updateRemoteAgentSettings(payload)
    editingRemoteAgentSetup.value = false

    const queuedTask = taskPendingRunnerSetup.value
    taskPendingRunnerSetup.value = null
    if (queuedTask) {
      window.setTimeout(() => {
        void startRemoteRun(queuedTask.task, queuedTask.preferredTool)
      }, 0)
    }
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function confirmRemoteCleanup() {
  cleaningUpRemoteArtifacts.value = true
  errorMessage.value = ''

  try {
    cleanupSummary.value = await cleanupRemoteAgentArtifacts()
    cleanupPendingConfirmation.value = false
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    cleaningUpRemoteArtifacts.value = false
  }
}

async function confirmRemoteReset() {
  resettingRemoteWorkspace.value = true
  errorMessage.value = ''

  try {
    resetSummary.value = await resetRemoteAgentWorkspace()
    resetPendingConfirmation.value = false
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    resettingRemoteWorkspace.value = false
  }
}

async function importLegacyTrackerData() {
  migrationImportPending.value = true
  errorMessage.value = ''

  try {
    migrationImportSummary.value = await importLegacyData()
    migrationStatus.value = null
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    migrationImportPending.value = false
  }
}

async function confirmDelete() {
  if (!taskPendingDeletion.value) {
    return
  }

  saving.value = true
  errorMessage.value = ''
  beginTaskLifecycleMutation(taskPendingDeletion.value.id, 'deleting')

  try {
    const deletedTaskId = taskPendingDeletion.value.id
    await deleteTask(deletedTaskId)
    taskPendingDeletion.value = null
    if (selectedTaskId.value === deletedTaskId) {
      closeTaskDrawer()
    }
    removeTaskRuns(deletedTaskId)
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
    clearTaskLifecycleMutation()
  }
}

async function confirmReviewDelete() {
  if (!reviewPendingDeletion.value) {
    return
  }

  saving.value = true
  errorMessage.value = ''

  try {
    const deletedReviewId = reviewPendingDeletion.value.id
    await deleteReview(deletedReviewId)
    reviewPendingDeletion.value = null
    removeReview(deletedReviewId)
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function startRemoteRun(
  task: Task,
  preferredTool: RemoteAgentPreferredTool = selectedTaskDispatchTool.value,
) {
  if (remoteAgentSettings.value === null) {
    try {
      await loadRemoteAgentSettings()
    } catch {
      // The user-facing error below remains the real fallback if the settings
      // endpoint is still unavailable after a best-effort sync.
    }
  }

  if (remoteAgentSettings.value && !remoteAgentSettings.value.configured) {
    errorMessage.value =
      'Remote dispatch is not configured yet. Run `track remote-agent configure --host <host> --user <user> --identity-file ~/.ssh/id_ed25519` locally first.'
    currentPage.value = 'settings'
    return
  }

  if (remoteAgentSettings.value && !runnerSetupReady.value) {
    taskPendingRunnerSetup.value = { task, preferredTool }
    editingRemoteAgentSetup.value = true
    currentPage.value = 'settings'
    return
  }

  dispatchingTaskId.value = task.id
  errorMessage.value = ''

  try {
    const dispatch = await dispatchTask(task.id, { preferredTool })
    upsertRunRecord(task, dispatch)
    upsertLatestTaskDispatch(dispatch)
    upsertSelectedTaskRun(task, dispatch)
  } catch (error) {
    await loadRuns().catch(() => undefined)
    setFriendlyError(error)
  } finally {
    dispatchingTaskId.value = null
  }
}

async function cancelRemoteRun(task: Task) {
  cancelingDispatchTaskId.value = task.id
  errorMessage.value = ''

  try {
    const dispatch = await cancelDispatch(task.id)
    upsertRunRecord(task, dispatch)
    upsertLatestTaskDispatch(dispatch)
    upsertSelectedTaskRun(task, dispatch)
  } catch (error) {
    await loadRuns().catch(() => undefined)
    setFriendlyError(error)
  } finally {
    cancelingDispatchTaskId.value = null
  }
}

async function cancelReviewRun(review: ReviewRecord) {
  cancelingReviewId.value = review.id
  errorMessage.value = ''

  try {
    const run = await cancelReview(review.id)
    upsertLatestReviewRun(review.id, run)
    upsertSelectedReviewRun(run)
  } catch (error) {
    setFriendlyError(error)
  } finally {
    cancelingReviewId.value = null
  }
}

async function submitReviewFollowUp(payload: ReviewFollowUpInput) {
  if (!followingUpReview.value) {
    return
  }

  followingUpReviewId.value = followingUpReview.value.id
  errorMessage.value = ''

  try {
    const run = await followUpReview(followingUpReview.value.id, payload)
    upsertReviewSummary(
      {
        ...followingUpReview.value,
        updatedAt: run.createdAt,
      },
      run,
    )
    upsertSelectedReviewRun(run)
    followingUpReview.value = null
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    followingUpReviewId.value = null
  }
}

async function discardRunHistory(task: Task) {
  discardingDispatchTaskId.value = task.id
  errorMessage.value = ''

  try {
    await discardDispatch(task.id)
    removeTaskRuns(task.id)
  } catch (error) {
    await loadRuns().catch(() => undefined)
    setFriendlyError(error)
  } finally {
    discardingDispatchTaskId.value = null
  }
}

async function submitFollowUp(payload: TaskFollowUpInput) {
  if (!followingUpTask.value) {
    return
  }

  followingUpTaskId.value = followingUpTask.value.id
  errorMessage.value = ''

  try {
    const dispatch = await followUpTask(followingUpTask.value.id, payload)
    upsertRunRecord(followingUpTask.value, dispatch)
    upsertLatestTaskDispatch(dispatch)
    upsertSelectedTaskRun(followingUpTask.value, dispatch)
    followingUpTask.value = null
    await refreshAll()
  } catch (error) {
    await loadRuns().catch(() => undefined)
    setFriendlyError(error)
  } finally {
    followingUpTaskId.value = null
  }
}

async function handlePrimaryAction() {
  if (!selectedTask.value) {
    return
  }

  const task = selectedTask.value
  const latestDispatch = selectedTaskLatestDispatch.value

  if (task.status === 'closed') {
    await updateTaskStatus(task, 'open')
    return
  }

  if (latestDispatch?.status === 'preparing' || latestDispatch?.status === 'running') {
    await cancelRemoteRun(task)
    return
  }

  if (selectedTaskCanContinue.value) {
    followingUpTask.value = task
    return
  }

  await startRemoteRun(task)
}

function openTaskEditor(task: Task) {
  editingTask.value = task
}

function openNewTaskEditor() {
  creatingTask.value = true
}

function openNewReviewEditor() {
  creatingReview.value = true
}

function openReviewFollowUpEditor(review = selectedReview.value) {
  if (!review) {
    return
  }

  followingUpReview.value = review
}

function openProjectEditor(project = selectedProjectDetails.value) {
  if (!project) {
    return
  }

  editingProject.value = project
}

function openRunnerSetup() {
  taskPendingRunnerSetup.value = null
  editingRemoteAgentSetup.value = true
}

function closeTaskEditor() {
  editingTask.value = null
  creatingTask.value = false
}

function closeReviewEditor() {
  creatingReview.value = false
}

function closeReviewFollowUpEditor() {
  followingUpReview.value = null
}

function closeProjectEditor() {
  editingProject.value = null
}

function closeRunnerSetup() {
  editingRemoteAgentSetup.value = false
  taskPendingRunnerSetup.value = null
}

function closeFollowUpEditor() {
  followingUpTask.value = null
}

function queueTaskDeletion(task: Task) {
  taskPendingDeletion.value = task
}

function clearPendingDeletion() {
  taskPendingDeletion.value = null
}

function queueReviewDeletion(review: ReviewRecord) {
  reviewPendingDeletion.value = review
}

function clearPendingReviewDeletion() {
  reviewPendingDeletion.value = null
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

// =============================================================================
// Background Sync
// =============================================================================
//
// Tasks still refresh quickly because local creation is a core workflow. Run
// status remains slower because those updates imply remote work and can happen
// in the background without forcing a noisy interface.
async function pollForTaskChanges() {
  if (
    taskChangePollInFlight ||
    loading.value ||
    refreshing.value ||
    saving.value ||
    dispatchingTaskId.value !== null ||
    cancelingDispatchTaskId.value !== null ||
    cancelingReviewId.value !== null ||
    discardingDispatchTaskId.value !== null ||
    followingUpTaskId.value !== null
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
    // Background refresh is a convenience path. Foreground actions already
    // surface actionable failures.
  } finally {
    taskChangePollInFlight = false
  }
}

async function pollForRunChanges() {
  if (
    runPollInFlight ||
    loading.value ||
    refreshing.value ||
    saving.value ||
    dispatchingTaskId.value !== null ||
    cancelingDispatchTaskId.value !== null ||
    cancelingReviewId.value !== null ||
    discardingDispatchTaskId.value !== null ||
    followingUpTaskId.value !== null
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
      loadSelectedTaskRunHistory().catch(() => {
        // The rest of the run state remains useful even if the drawer history
        // fails to refresh on one background poll.
      }),
      loadSelectedReviewRunHistory().catch(() => {
        // Review history is secondary to the latest status cards, so this poll
        // stays best-effort for the review drawer as well.
      }),
    ])
  } catch {
    // The last known run state remains useful, so this poll stays best-effort.
  } finally {
    runPollInFlight = false
  }
}

watch([showClosed, selectedProjectFilter], () => {
  if (loading.value) {
    return
  }

  void (async () => {
    try {
      await loadTasks()
      await Promise.all([
        loadLatestDispatchesForVisibleTasks(),
        loadSelectedTaskRunHistory().catch(() => {
          // Changing filters should not blank the queue just because drawer
          // history could not be refreshed for the currently selected task.
        }),
      ])
    } catch (error) {
      setFriendlyError(error)
    }
  })()
})

watch(
  tasks,
  (nextTasks) => {
    if (pendingSelectedTaskId.value) {
      const pendingTask = nextTasks.find((task) => task.id === pendingSelectedTaskId.value)
      if (pendingTask) {
        selectedTaskId.value = pendingTask.id
        pendingSelectedTaskId.value = null
        isTaskDrawerOpen.value = true
        return
      }
    }

    if (selectedTaskId.value && !nextTasks.some((task) => task.id === selectedTaskId.value)) {
      closeTaskDrawer()
    }
  },
  { immediate: true },
)

watch(
  reviews,
  (nextReviews) => {
    if (
      selectedReviewId.value &&
      !nextReviews.some((summary) => summary.review.id === selectedReviewId.value)
    ) {
      closeReviewDrawer()
    }
  },
  { immediate: true },
)

watch(
  availableProjects,
  (nextProjects) => {
    if (
      !selectedProjectDetailsId.value ||
      !nextProjects.some((project) => project.canonicalName === selectedProjectDetailsId.value)
    ) {
      selectedProjectDetailsId.value = nextProjects[0]?.canonicalName ?? null
    }

    if (
      selectedProjectFilter.value &&
      !nextProjects.some((project) => project.canonicalName === selectedProjectFilter.value)
    ) {
      selectedProjectFilter.value = ''
    }
  },
  { immediate: true },
)

watch(currentPage, (nextPage) => {
  if (nextPage !== 'tasks') {
    isTaskDrawerOpen.value = false
    selectedTaskRuns.value = []
  }

  if (nextPage !== 'reviews') {
    isReviewDrawerOpen.value = false
    selectedReviewRuns.value = []
    followingUpReview.value = null
  }
})

watch(
  selectedTaskId,
  () => {
    selectedTaskStartTool.value = defaultRemoteAgentPreferredTool.value
  },
  { immediate: true },
)

watch(defaultRemoteAgentPreferredTool, (nextTool, previousTool) => {
  if (selectedTaskStartTool.value === previousTool) {
    selectedTaskStartTool.value = nextTool
  }
})

watch([isTaskDrawerOpen, selectedTask], ([drawerOpen, task]) => {
  if (!task) {
    isTaskDrawerOpen.value = false
    selectedTaskRuns.value = []
    return
  }

  if (!drawerOpen) {
    selectedTaskRuns.value = []
    return
  }

  void loadSelectedTaskRunHistory().catch(setFriendlyError)
})

watch([isReviewDrawerOpen, selectedReview], ([drawerOpen, review]) => {
  if (!review) {
    isReviewDrawerOpen.value = false
    selectedReviewRuns.value = []
    return
  }

  if (!drawerOpen) {
    selectedReviewRuns.value = []
    return
  }

  void loadSelectedReviewRunHistory().catch(setFriendlyError)
})

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
</script>

<template>
  <main class="min-h-screen px-4 py-4 sm:px-6 sm:py-6 lg:px-8">
    <div class="mx-auto max-w-[1800px]">
      <div class="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
        <aside class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel lg:sticky lg:top-4 lg:self-start">
          <div class="flex items-center justify-between gap-3 border-b border-fg2/10 pb-4">
            <p class="font-display text-3xl text-fg0">
              track
            </p>

            <span
              class="border px-3 py-2 text-xs font-semibold tracking-[0.08em]"
              :class="
                runnerSetupReady
                  ? 'border-aqua/30 bg-aqua/10 text-aqua'
                  : remoteAgentSettings?.configured
                    ? 'border-yellow/30 bg-yellow/10 text-yellow'
                    : 'border-fg2/20 bg-bg0 text-fg2'
              "
            >
              {{
                runnerSetupReady
                  ? 'ready'
                  : remoteAgentSettings?.configured
                    ? 'setup'
                    : 'local'
              }}
            </span>
          </div>

          <nav class="mt-4 space-y-2">
            <button
              type="button"
              class="flex w-full items-center justify-between border px-3 py-3 text-left text-sm tracking-[0.08em] transition"
              :class="
                currentPage === 'tasks'
                  ? 'border-aqua/35 bg-aqua/10 text-aqua'
                  : 'border-fg2/20 bg-bg0 text-fg1 hover:border-fg1/35 hover:text-fg0'
              "
              @click="currentPage = 'tasks'"
            >
              <span>Tasks</span>
              <span class="text-xs text-fg3">{{ visibleTaskCount }}</span>
            </button>
            <button
              type="button"
              class="flex w-full items-center justify-between border px-3 py-3 text-left text-sm tracking-[0.08em] transition"
              :class="
                currentPage === 'reviews'
                  ? 'border-aqua/35 bg-aqua/10 text-aqua'
                  : 'border-fg2/20 bg-bg0 text-fg1 hover:border-fg1/35 hover:text-fg0'
              "
              @click="currentPage = 'reviews'"
            >
              <span>Reviews</span>
              <span class="text-xs text-fg3">{{ reviewCount }}</span>
            </button>
            <button
              type="button"
              class="flex w-full items-center justify-between border px-3 py-3 text-left text-sm tracking-[0.08em] transition"
              :class="
                currentPage === 'runs'
                  ? 'border-aqua/35 bg-aqua/10 text-aqua'
                  : 'border-fg2/20 bg-bg0 text-fg1 hover:border-fg1/35 hover:text-fg0'
              "
              @click="currentPage = 'runs'"
            >
              <span>Runs</span>
              <span class="text-xs text-fg3">{{ activeRemoteWorkCount }}</span>
            </button>
            <button
              type="button"
              class="flex w-full items-center justify-between border px-3 py-3 text-left text-sm tracking-[0.08em] transition"
              :class="
                currentPage === 'projects'
                  ? 'border-aqua/35 bg-aqua/10 text-aqua'
                  : 'border-fg2/20 bg-bg0 text-fg1 hover:border-fg1/35 hover:text-fg0'
              "
              @click="currentPage = 'projects'"
            >
              <span>Projects</span>
              <span class="text-xs text-fg3">{{ totalProjectCount }}</span>
            </button>
            <button
              type="button"
              class="flex w-full items-center justify-between border px-3 py-3 text-left text-sm tracking-[0.08em] transition"
              :class="
                currentPage === 'settings'
                  ? 'border-aqua/35 bg-aqua/10 text-aqua'
                  : 'border-fg2/20 bg-bg0 text-fg1 hover:border-fg1/35 hover:text-fg0'
              "
              @click="currentPage = 'settings'"
            >
              <span>Settings</span>
              <span class="text-xs text-fg3">remote</span>
            </button>
          </nav>

          <div class="mt-6 border-t border-fg2/10 pt-4 text-sm text-fg2">
            <div class="flex items-center justify-between">
              <span>Active remote work</span>
              <span>{{ activeRemoteWorkCount }}</span>
            </div>
            <div class="mt-2 flex items-center justify-between">
              <span>Visible tasks</span>
              <span>{{ visibleTaskCount }}</span>
            </div>
            <div class="mt-2 flex items-center justify-between">
              <span>Reviews</span>
              <span>{{ reviewCount }}</span>
            </div>
            <div class="mt-2 flex items-center justify-between">
              <span>Projects</span>
              <span>{{ totalProjectCount }}</span>
            </div>
          </div>
        </aside>

        <section class="min-w-0 space-y-4">
          <div
            v-if="errorMessage"
            data-testid="error-banner"
            class="border border-red/30 bg-red/10 px-4 py-3 text-sm text-red shadow-panel"
          >
            {{ errorMessage }}
          </div>

          <div
            v-if="loading"
            class="border border-fg2/20 bg-bg1/95 px-5 py-16 text-center text-sm text-fg3 shadow-panel"
          >
            Loading tracker data...
          </div>

          <template v-else>
            <section v-if="migrationImportSummary" class="space-y-4">
              <div class="border border-green/25 bg-green/8 p-4 text-sm leading-7 text-green shadow-panel">
                Imported {{ migrationImportSummary.importedTasks }} tasks, {{ migrationImportSummary.importedProjects }} projects, and {{ migrationImportSummary.importedReviews }} reviews into the SQLite backend.
              </div>

              <div
                v-if="migrationImportSummary.cleanupCandidates.length > 0"
                class="border border-fg2/15 bg-bg1/95 p-4 shadow-panel"
              >
                <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Optional legacy cleanup
                </p>
                <p class="mt-3 text-sm leading-7 text-fg2">
                  After you confirm the imported data looks correct, run these commands on the host. Start with <code class="font-mono text-fg1">track configure</code> so the CLI materializes <code class="font-mono text-fg1">~/.config/track/cli.json</code>, then reinstall <code class="font-mono text-fg1">cargo-airbender</code> from your <code class="font-mono text-fg1">airbender-platform</code> checkout.
                </p>
                <div class="mt-4 overflow-x-auto border border-fg2/10 bg-bg0/60 px-4 py-4 font-mono text-xs leading-7 text-fg1">
                  <p>track configure</p>
                  <p>cargo install --path crates/cargo-airbender --force</p>
                  <p
                    v-for="candidate in migrationImportSummary.cleanupCandidates"
                    :key="candidate.path"
                  >
                    {{ migrationCleanupCommand(candidate.path) }}
                  </p>
                </div>
                <p class="mt-3 text-sm leading-7 text-fg3">
                  Keep <code class="font-mono text-fg1">~/.track/models</code> if you use local capture.
                </p>
              </div>
            </section>

            <section v-if="migrationRequired && migrationStatus" class="space-y-4">
              <div class="border border-yellow/25 bg-yellow/8 p-5 shadow-panel">
                <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-yellow">
                  Migration required
                </p>
                <h1 class="mt-3 font-display text-3xl text-fg0 sm:text-4xl">
                  Import legacy track data before using the app
                </h1>
                <p class="mt-4 max-w-3xl text-sm leading-7 text-fg2">
                  This backend uses SQLite-backed state. Legacy Markdown and JSON data were detected, so normal API routes stay gated until that data is imported.
                </p>

                <div class="mt-6 grid gap-4 md:grid-cols-3">
                  <div class="border border-fg2/15 bg-bg0/60 p-4">
                    <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">Projects</p>
                    <p class="mt-2 text-2xl text-fg0">{{ migrationStatus.summary.projectsFound }}</p>
                  </div>
                  <div class="border border-fg2/15 bg-bg0/60 p-4">
                    <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">Tasks</p>
                    <p class="mt-2 text-2xl text-fg0">{{ migrationStatus.summary.tasksFound }}</p>
                  </div>
                  <div class="border border-fg2/15 bg-bg0/60 p-4">
                    <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">Reviews</p>
                    <p class="mt-2 text-2xl text-fg0">{{ migrationStatus.summary.reviewsFound }}</p>
                  </div>
                </div>

                <div class="mt-6 flex flex-wrap gap-3">
                  <button
                    type="button"
                    class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:cursor-not-allowed disabled:opacity-60"
                    :disabled="migrationImportPending || !migrationStatus.canImport"
                    @click="importLegacyTrackerData"
                  >
                    {{ migrationImportPending ? 'Importing...' : 'Import legacy data' }}
                  </button>
                </div>
              </div>

              <div
                v-if="migrationStatus.skippedRecords.length > 0"
                class="border border-fg2/15 bg-bg1/95 p-4 shadow-panel"
              >
                <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Skipped legacy records
                </p>
                <ul class="mt-4 space-y-3 text-sm leading-6 text-fg2">
                  <li
                    v-for="record in migrationStatus.skippedRecords.slice(0, 5)"
                    :key="`${record.kind}:${record.path}`"
                    class="border border-fg2/10 bg-bg0/50 px-3 py-3"
                  >
                    <p class="font-semibold text-fg1">{{ record.kind }}</p>
                    <p class="mt-1 break-all">{{ record.path }}</p>
                    <p class="mt-2 text-fg3">{{ record.error }}</p>
                  </li>
                </ul>
              </div>
            </section>

            <section v-else-if="currentPage === 'tasks'" class="space-y-4">
              <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
                  <div>
                    <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
                      Tasks
                    </h1>
                  </div>

                  <div class="flex flex-wrap items-center gap-3">
                    <select
                      v-model="selectedProjectFilter"
                      class="min-w-[220px] border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
                    >
                      <option value="">
                        All projects
                      </option>
                      <option
                        v-for="project in availableProjects"
                        :key="project.canonicalName"
                        :value="project.canonicalName"
                      >
                        {{ project.canonicalName }}
                      </option>
                    </select>

                    <label class="flex items-center gap-3 border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg1">
                      <input
                        v-model="showClosed"
                        type="checkbox"
                        class="h-4 w-4 border-fg2/30 bg-bg0 text-aqua focus:ring-aqua/50"
                      />
                      Closed
                    </label>

                    <button
                      type="button"
                      class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                      @click="openNewTaskEditor"
                    >
                      New task
                    </button>
                  </div>
                </div>
              </div>

              <div v-if="tasks.length === 0" class="border border-fg2/20 bg-bg1/95 px-4 py-12 text-center shadow-panel">
                  <p class="font-display text-2xl text-fg0">
                    Queue is empty.
                  </p>
                  <p class="mt-3 text-sm leading-6 text-fg2">
                    New tasks from the CLI or the web form will appear here.
                  </p>
              </div>

              <div v-else class="space-y-4">
                <section
                  v-for="group in taskGroups"
                  :key="group.project"
                  :data-project="group.project"
                  data-testid="task-group"
                  class="overflow-hidden border border-fg2/20 bg-bg1/95 shadow-panel"
                >
                  <div class="border-b border-fg2/10 bg-bg0/35 px-4 py-3">
                    <div class="flex items-center justify-between gap-3">
                      <p class="text-[11px] font-semibold uppercase tracking-[0.22em] text-fg2">
                        {{ group.project }}
                      </p>
                      <span class="text-xs text-fg3">{{ group.tasks.length }}</span>
                    </div>
                  </div>

                  <div class="divide-y divide-fg2/10">
                    <button
                      v-for="task in group.tasks"
                      :key="task.id"
                      type="button"
                      :data-task-id="task.id"
                      data-testid="task-row"
                      class="w-full px-4 py-5 text-left transition hover:bg-bg0/40"
                      :class="selectedTask?.id === task.id && isTaskDrawerOpen ? 'bg-bg0/55' : 'bg-transparent'"
                      @click="selectTask(task.id)"
                    >
                      <div class="space-y-3">
                        <p class="text-xs tracking-[0.08em] text-fg3">
                          {{ task.source ?? 'manual' }} / {{ taskReference(task) }}
                        </p>

                        <p class="whitespace-pre-wrap text-xl leading-8 text-fg0">
                          {{ taskTitle(task) }}
                        </p>

                        <div class="flex flex-wrap items-center gap-2">
                          <span
                            class="border px-2 py-1 text-[11px] font-semibold tracking-[0.08em]"
                            :class="priorityBadgeClass(task.priority)"
                          >
                            {{ task.priority }}
                          </span>
                          <span
                            class="border px-2 py-1 text-[11px] font-semibold tracking-[0.08em]"
                            :class="taskStatusBadgeClass(task.status)"
                          >
                            {{ task.status }}
                          </span>
                          <span
                            class="border px-2 py-1 text-[11px] font-semibold tracking-[0.08em]"
                            :class="dispatchBadgeClass(latestDispatchByTaskId[task.id])"
                          >
                            {{ dispatchStatusLabel(latestDispatchByTaskId[task.id]) }}
                          </span>
                          <span class="text-xs tracking-[0.08em] text-fg3">
                            {{ formatTaskTimestamp(task) }}
                          </span>
                        </div>
                      </div>
                    </button>
                  </div>
                </section>
              </div>
            </section>

            <section v-else-if="currentPage === 'reviews'" class="space-y-4">
              <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
                  <div>
                    <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
                      Reviews
                    </h1>
                    <p class="mt-2 text-sm text-fg3">
                      Standalone PR reviews with persisted history and cleanup.
                    </p>
                  </div>

                  <div class="flex flex-wrap items-center gap-3">
                    <button
                      type="button"
                      class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:cursor-not-allowed disabled:opacity-60"
                      :disabled="!canRequestReview"
                      @click="openNewReviewEditor"
                    >
                      Request review
                    </button>
                    <button
                      v-if="reviewRequestDisabledReason"
                      type="button"
                      class="border border-fg2/20 bg-bg0 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                      @click="currentPage = 'settings'"
                    >
                      Open settings
                    </button>
                  </div>
                </div>
              </div>

              <div
                v-if="reviewRequestDisabledReason"
                class="border border-yellow/25 bg-yellow/8 px-4 py-3 text-sm leading-6 text-yellow shadow-panel"
              >
                {{ reviewRequestDisabledReason }}
              </div>

              <div v-if="reviews.length === 0" class="border border-fg2/20 bg-bg1/95 px-4 py-12 text-center shadow-panel">
                <p class="font-display text-2xl text-fg0">
                  No PR reviews yet.
                </p>
                <p class="mt-3 text-sm leading-6 text-fg2">
                  Request a review from a GitHub PR URL and it will show up here with its run history.
                </p>
              </div>

              <div v-else class="space-y-4">
                <article
                  v-for="summary in reviews"
                  :key="summary.review.id"
                  :data-review-id="summary.review.id"
                  class="border border-fg2/20 bg-bg1/95 shadow-panel transition hover:border-fg1/25"
                >
                  <button
                    type="button"
                    data-testid="review-row"
                    class="w-full px-4 py-5 text-left transition hover:bg-bg0/35"
                    @click="selectReview(summary.review.id)"
                  >
                    <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                      <div class="min-w-0">
                        <p class="text-xs tracking-[0.08em] text-fg3">
                          {{ summary.review.repositoryFullName }} / PR #{{ summary.review.pullRequestNumber }}
                        </p>
                        <h2 class="mt-3 whitespace-pre-wrap text-xl leading-8 text-fg0">
                          {{ summary.review.pullRequestTitle }}
                        </h2>
                        <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                          <span class="border px-2 py-1" :class="dispatchBadgeClass(summary.latestRun)">
                            {{ dispatchStatusLabel(summary.latestRun) }}
                          </span>
                          <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                            @{{ summary.review.mainUser }}
                          </span>
                          <span class="text-fg3">Created {{ formatDateTime(summary.review.createdAt) }}</span>
                          <span v-if="summary.latestRun?.reviewSubmitted" class="text-green">
                            Review submitted
                          </span>
                        </div>
                        <p class="mt-4 text-sm leading-7 text-fg2">
                          {{ dispatchSummary(summary.latestRun, 'review') }}
                        </p>
                      </div>
                    </div>
                  </button>
                </article>
              </div>
            </section>

            <section v-else-if="currentPage === 'runs'" class="space-y-4">
              <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
                  Runs
                </h1>
                <p class="mt-2 text-sm text-fg3">
                  Active agents and recent outcomes
                </p>
              </div>

              <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                      Active task runs
                    </p>
                    <p class="mt-2 text-sm text-fg2">
                      Task agents that are still preparing or actively running.
                    </p>
                  </div>
                  <span class="text-xs text-fg3">{{ activeRuns.length }}</span>
                </div>

                <div
                  v-if="activeRuns.length === 0"
                  class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
                >
                  No task runs are active at the moment.
                </div>

                <div v-else class="mt-4 space-y-3">
                  <article
                    v-for="run in activeRuns"
                    :key="run.dispatch.dispatchId"
                    class="border border-fg2/15 bg-bg0/60 p-4"
                  >
                    <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                      <div class="min-w-0">
                        <p class="text-xs tracking-[0.08em] text-fg3">
                          {{ run.task.project }}
                        </p>
                        <h2 class="mt-3 whitespace-pre-wrap text-xl leading-8 text-fg0">
                          {{ taskTitle(run.task) }}
                        </h2>
                        <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                          <span class="border px-2 py-1" :class="dispatchBadgeClass(run.dispatch)">
                            {{ dispatchStatusLabel(run.dispatch) }}
                          </span>
                          <span class="text-fg3">Started {{ formatDateTime(run.dispatch.createdAt) }}</span>
                        </div>
                        <p class="mt-4 text-sm leading-7 text-fg2">
                          {{ dispatchSummary(run.dispatch) }}
                        </p>
                      </div>

                      <div class="flex shrink-0 flex-wrap gap-2">
                        <button
                          type="button"
                          class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                          @click="openTaskFromRun(run)"
                        >
                          Open task
                        </button>
                        <button
                          v-if="run.dispatch.pullRequestUrl"
                          type="button"
                          class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                          @click="openExternal(run.dispatch.pullRequestUrl)"
                        >
                          View PR
                        </button>
                      </div>
                    </div>
                  </article>
                </div>
              </section>

              <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                      Active PR reviews
                    </p>
                    <p class="mt-2 text-sm text-fg2">
                      Standalone review runs that are still preparing or actively running.
                    </p>
                  </div>
                  <span class="text-xs text-fg3">{{ activeReviewRuns.length }}</span>
                </div>

                <div
                  v-if="activeReviewRuns.length === 0"
                  class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
                >
                  No PR reviews are running right now.
                </div>

                <div v-else class="mt-4 space-y-3">
                  <article
                    v-for="summary in activeReviewRuns"
                    :key="summary.latestRun?.dispatchId ?? summary.review.id"
                    class="border border-fg2/15 bg-bg0/60 p-4"
                  >
                    <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                      <div class="min-w-0">
                        <p class="text-xs tracking-[0.08em] text-fg3">
                          {{ summary.review.repositoryFullName }}
                        </p>
                        <h2 class="mt-3 whitespace-pre-wrap text-xl leading-8 text-fg0">
                          {{ summary.review.pullRequestTitle }}
                        </h2>
                        <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                          <span class="border px-2 py-1" :class="dispatchBadgeClass(summary.latestRun)">
                            {{ dispatchStatusLabel(summary.latestRun) }}
                          </span>
                          <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                            @{{ summary.review.mainUser }}
                          </span>
                          <span class="text-fg3">
                            Started {{ formatDateTime(summary.latestRun?.createdAt ?? summary.review.createdAt) }}
                          </span>
                        </div>
                        <p class="mt-4 text-sm leading-7 text-fg2">
                          {{ dispatchSummary(summary.latestRun, 'review') }}
                        </p>
                      </div>

                      <div class="flex shrink-0 flex-wrap gap-2">
                        <button
                          type="button"
                          class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                          @click="selectReview(summary.review.id)"
                        >
                          Open review
                        </button>
                        <button
                          type="button"
                          class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                          @click="openExternal(summary.review.pullRequestUrl)"
                        >
                          View PR
                        </button>
                      </div>
                    </div>
                  </article>
                </div>
              </section>

              <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                      Recent runs
                    </p>
                    <p class="mt-2 text-sm text-fg2">
                      The latest dispatch results across all tasks, including follow-ups and failures.
                    </p>
                  </div>
                  <span class="text-xs text-fg3">{{ recentRuns.length }}</span>
                </div>

                <div
                  v-if="recentRuns.length === 0"
                  class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
                >
                  No dispatch history has been recorded yet.
                </div>

                <div v-else class="mt-4 space-y-3">
                  <article
                    v-for="run in recentRuns"
                    :key="run.dispatch.dispatchId"
                    class="border border-fg2/15 bg-bg0/60 p-4"
                  >
                    <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                      <div class="min-w-0">
                        <p class="text-xs tracking-[0.08em] text-fg3">
                          {{ run.task.project }}
                        </p>
                        <h2 class="mt-3 whitespace-pre-wrap text-lg leading-8 text-fg0">
                          {{ taskTitle(run.task) }}
                        </h2>
                        <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                          <span class="border px-2 py-1" :class="dispatchBadgeClass(run.dispatch)">
                            {{ dispatchStatusLabel(run.dispatch) }}
                          </span>
                          <span class="text-fg3">Started {{ formatDateTime(run.dispatch.createdAt) }}</span>
                          <span v-if="run.dispatch.finishedAt" class="text-fg3">
                            • Finished {{ formatDateTime(run.dispatch.finishedAt) }}
                          </span>
                        </div>
                        <p class="mt-4 text-sm leading-7 text-fg2">
                          {{ dispatchSummary(run.dispatch) }}
                        </p>
                      </div>

                      <div class="flex shrink-0 flex-wrap gap-2">
                        <button
                          type="button"
                          class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                          @click="openTaskFromRun(run)"
                        >
                          Open task
                        </button>
                        <button
                          v-if="run.dispatch.pullRequestUrl"
                          type="button"
                          class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                          @click="openExternal(run.dispatch.pullRequestUrl)"
                        >
                          View PR
                        </button>
                      </div>
                    </div>
                  </article>
                </div>
              </section>

              <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                      Recent PR reviews
                    </p>
                    <p class="mt-2 text-sm text-fg2">
                      The latest standalone review outcomes, including submitted reviews and failures.
                    </p>
                  </div>
                  <span class="text-xs text-fg3">{{ recentReviewRuns.length }}</span>
                </div>

                <div
                  v-if="recentReviewRuns.length === 0"
                  class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
                >
                  No PR review history has been recorded yet.
                </div>

                <div v-else class="mt-4 space-y-3">
                  <article
                    v-for="summary in recentReviewRuns"
                    :key="summary.latestRun?.dispatchId ?? summary.review.id"
                    class="border border-fg2/15 bg-bg0/60 p-4"
                  >
                    <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                      <div class="min-w-0">
                        <p class="text-xs tracking-[0.08em] text-fg3">
                          {{ summary.review.repositoryFullName }}
                        </p>
                        <h2 class="mt-3 whitespace-pre-wrap text-lg leading-8 text-fg0">
                          {{ summary.review.pullRequestTitle }}
                        </h2>
                        <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                          <span class="border px-2 py-1" :class="dispatchBadgeClass(summary.latestRun)">
                            {{ dispatchStatusLabel(summary.latestRun) }}
                          </span>
                          <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                            @{{ summary.review.mainUser }}
                          </span>
                          <span class="text-fg3">
                            Started {{ formatDateTime(summary.latestRun?.createdAt ?? summary.review.createdAt) }}
                          </span>
                          <span v-if="summary.latestRun?.finishedAt" class="text-fg3">
                            • Finished {{ formatDateTime(summary.latestRun?.finishedAt) }}
                          </span>
                          <span v-if="summary.latestRun?.reviewSubmitted" class="text-green">
                            Review submitted
                          </span>
                        </div>
                        <p class="mt-4 text-sm leading-7 text-fg2">
                          {{ dispatchSummary(summary.latestRun, 'review') }}
                        </p>
                      </div>

                      <div class="flex shrink-0 flex-wrap gap-2">
                        <button
                          type="button"
                          class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                          @click="selectReview(summary.review.id)"
                        >
                          Open review
                        </button>
                        <button
                          type="button"
                          class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                          @click="openExternal(summary.review.pullRequestUrl)"
                        >
                          View PR
                        </button>
                      </div>
                    </div>
                  </article>
                </div>
              </section>
            </section>

            <section v-else-if="currentPage === 'projects'" class="space-y-4">
              <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
                  Projects
                </h1>
                <p class="mt-2 text-sm text-fg3">
                  Repository metadata for automation
                </p>
              </div>

              <div class="grid gap-4 xl:grid-cols-[minmax(280px,360px)_minmax(0,1fr)]">
                <section class="border border-fg2/20 bg-bg1/95 shadow-panel">
                  <div class="border-b border-fg2/10 px-4 py-3">
                    <div class="flex items-center justify-between gap-3">
                      <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                        Projects
                      </p>
                      <span class="text-xs text-fg3">{{ totalProjectCount }}</span>
                    </div>
                  </div>

                  <div v-if="availableProjects.length === 0" class="px-4 py-12 text-center">
                    <p class="font-display text-2xl text-fg0">
                      No projects yet.
                    </p>
                    <p class="mt-3 text-sm leading-6 text-fg2">
                      Projects appear after the CLI registers them with the backend.
                    </p>
                  </div>

                  <div v-else class="divide-y divide-fg2/10">
                    <button
                      v-for="project in availableProjects"
                      :key="project.canonicalName"
                      type="button"
                      class="w-full px-4 py-4 text-left transition hover:bg-bg0/40"
                      :class="selectedProjectDetails?.canonicalName === project.canonicalName ? 'bg-bg0/55' : 'bg-transparent'"
                      @click="selectedProjectDetailsId = project.canonicalName"
                    >
                      <p class="text-base text-fg0">
                        {{ project.canonicalName }}
                      </p>
                      <p class="mt-2 text-xs tracking-[0.08em] text-fg3">
                        {{ project.metadata?.repoUrl || 'Repository metadata is available through the backend only.' }}
                      </p>
                    </button>
                  </div>
                </section>

                <section class="border border-fg2/20 bg-bg1/95 shadow-panel">
                  <div v-if="selectedProjectDetails" class="space-y-6 p-4 sm:p-5">
                    <div class="border-b border-fg2/10 pb-4">
                      <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                        <div class="min-w-0">
                          <h2 class="font-display text-3xl text-fg0 sm:text-4xl">
                            {{ selectedProjectDetails.canonicalName }}
                          </h2>
                          <p class="mt-3 break-all text-sm leading-7 text-fg2">
                            {{ selectedProjectDetails.metadata?.repoUrl || 'No repository URL has been saved yet.' }}
                          </p>
                        </div>

                        <button
                          type="button"
                          class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                          @click="openProjectEditor(selectedProjectDetails)"
                        >
                          Edit metadata
                        </button>
                      </div>
                    </div>

                    <div class="grid gap-4 xl:grid-cols-2">
                      <section class="border border-fg2/15 bg-bg0/60 p-4">
                        <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                          Repository links
                        </p>
                        <dl class="mt-4 space-y-4 text-sm">
                          <div>
                            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                              Repo URL
                            </dt>
                            <dd class="mt-1 break-all text-fg1">
                              {{ selectedProjectDetails.metadata?.repoUrl || 'Not set' }}
                            </dd>
                          </div>
                          <div>
                            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                              Git URL
                            </dt>
                            <dd class="mt-1 break-all text-fg1">
                              {{ selectedProjectDetails.metadata?.gitUrl || 'Not set' }}
                            </dd>
                          </div>
                          <div>
                            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                              Base branch
                            </dt>
                            <dd class="mt-1 text-fg1">
                              {{ selectedProjectDetails.metadata?.baseBranch || 'Not set' }}
                            </dd>
                          </div>
                        </dl>
                      </section>

                      <section class="border border-fg2/15 bg-bg0/60 p-4">
                        <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                          Notes
                        </p>
                        <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                          {{ selectedProjectDetails.metadata?.description || 'No project description yet.' }}
                        </div>
                        <div
                          v-if="selectedProjectDetails.aliases.length > 0"
                          class="mt-4 border-t border-fg2/10 pt-4"
                        >
                          <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                            Aliases
                          </p>
                          <div class="mt-3 flex flex-wrap gap-2 text-[11px] font-semibold tracking-[0.08em]">
                            <span
                              v-for="alias in selectedProjectDetails.aliases"
                              :key="alias"
                              class="border border-fg2/15 bg-bg1 px-2 py-1 text-fg2"
                            >
                              {{ alias }}
                            </span>
                          </div>
                        </div>
                      </section>
                    </div>
                  </div>

                  <div v-else class="flex min-h-[360px] items-center justify-center px-6 py-12 text-center">
                    <div>
                      <p class="font-display text-2xl text-fg0 sm:text-3xl">
                        Select a project
                      </p>
                      <p class="mt-3 max-w-md text-sm leading-6 text-fg2">
                        Project metadata lives here so the queue can stay focused on tasks instead of repository configuration.
                      </p>
                    </div>
                  </div>
                </section>
              </div>
            </section>

            <section v-else class="space-y-4">
              <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
                  Settings
                </h1>
                <p class="mt-2 text-sm text-fg3">
                  Remote runner configuration for task dispatches and PR reviews
                </p>
              </div>

              <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                  <span
                    class="border px-2 py-1"
                    :class="
                      runnerSetupReady
                        ? 'border-aqua/30 bg-aqua/10 text-aqua'
                        : remoteAgentSettings?.configured
                          ? 'border-yellow/30 bg-yellow/10 text-yellow'
                          : 'border-fg2/20 bg-bg0 text-fg2'
                    "
                  >
                    {{
                      runnerSetupReady
                        ? 'Runner ready'
                        : remoteAgentSettings?.configured
                          ? 'Runner needs shell prelude'
                          : 'Remote dispatch not configured'
                    }}
                  </span>
                </div>

                <div class="mt-5 grid gap-4 md:grid-cols-2 xl:grid-cols-4 2xl:grid-cols-8">
                  <dl class="contents">
                    <div class="border border-fg2/15 bg-bg0/60 p-4">
                      <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                        Host
                      </dt>
                      <dd class="mt-2 break-all text-sm text-fg1">
                        {{ remoteAgentSettings?.host || 'Not configured' }}
                      </dd>
                    </div>
                    <div class="border border-fg2/15 bg-bg0/60 p-4">
                      <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                        User
                      </dt>
                      <dd class="mt-2 break-all text-sm text-fg1">
                        {{ remoteAgentSettings?.user || 'Not configured' }}
                      </dd>
                    </div>
                    <div class="border border-fg2/15 bg-bg0/60 p-4">
                      <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                        Port
                      </dt>
                      <dd class="mt-2 text-sm text-fg1">
                        {{ remoteAgentSettings?.port ?? 22 }}
                      </dd>
                    </div>
                    <div class="border border-fg2/15 bg-bg0/60 p-4">
                      <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                        Shell prelude
                      </dt>
                      <dd class="mt-2 text-sm text-fg1">
                        {{ runnerSetupReady ? 'Configured' : 'Missing' }}
                      </dd>
                    </div>
                    <div class="border border-fg2/15 bg-bg0/60 p-4">
                      <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                        Preferred tool
                      </dt>
                      <dd class="mt-2 text-sm text-fg1">
                        {{ remoteAgentSettings?.preferredTool === 'claude' ? 'Claude' : 'Codex' }}
                      </dd>
                    </div>
                    <div class="border border-fg2/15 bg-bg0/60 p-4">
                      <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                        Automatic follow-up
                      </dt>
                      <dd class="mt-2 text-sm text-fg1">
                        {{ remoteAgentSettings?.reviewFollowUp?.enabled ? 'Enabled' : 'Disabled' }}
                      </dd>
                    </div>
                    <div class="border border-fg2/15 bg-bg0/60 p-4">
                      <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                        Main user
                      </dt>
                      <dd class="mt-2 text-sm text-fg1">
                        {{ remoteAgentSettings?.reviewFollowUp?.mainUser || 'Not set' }}
                      </dd>
                    </div>
                  </dl>

                  <button
                    type="button"
                    class="flex h-full items-center justify-center border border-aqua/35 bg-aqua/10 px-4 py-3 text-center text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                    @click="openRunnerSetup"
                  >
                    Edit runner setup
                  </button>
                </div>

                <div class="mt-6 space-y-4">
                  <section class="border border-fg2/15 bg-bg0/60 p-4">
                    <div class="flex items-start justify-between gap-4">
                      <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                        Current shell prelude
                      </p>
                      <span
                        :title="shellPreludeHelpText"
                        aria-label="Why the shell prelude exists"
                        tabindex="0"
                        class="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-fg2/20 bg-bg1/60 text-xs font-semibold text-fg2 transition hover:border-fg1/35 hover:text-fg0 focus:border-aqua/50 focus:text-fg0 focus:outline-none"
                      >
                        i
                      </span>
                    </div>
                    <pre class="mt-4 overflow-x-auto whitespace-pre-wrap text-sm leading-7 text-fg1">{{ remoteAgentSettings?.shellPrelude || 'No shell prelude has been saved yet.' }}</pre>
                  </section>

                  <section class="border border-fg2/15 bg-bg0/60 p-4">
                    <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                      Default review prompt
                    </p>
                    <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                      {{ remoteAgentSettings?.reviewFollowUp?.defaultReviewPrompt || 'Not set' }}
                    </div>
                  </section>
                </div>

                <section class="mt-4 border border-fg2/15 bg-bg0/60 p-4">
                  <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                    <div class="min-w-0">
                      <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                        Manual cleanup
                      </p>
                      <div class="mt-4 space-y-4 text-sm leading-7 text-fg1">
                        <p>
                          Sweep the remote workspace for stale task artifacts that are no longer needed.
                        </p>
                        <p>
                          Open tasks keep their tracked worktrees. Closed tasks keep metadata but release worktrees. Missing tasks lose both remote artifacts and their saved local dispatch history.
                        </p>
                      </div>
                    </div>

                    <button
                      type="button"
                      data-testid="settings-cleanup-button"
                      class="border border-orange/30 bg-orange/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-orange transition hover:bg-orange/15 disabled:cursor-not-allowed disabled:opacity-60"
                      :disabled="cleaningUpRemoteArtifacts || !remoteAgentSettings?.configured"
                      @click="openRemoteCleanupConfirmation"
                    >
                      {{ cleaningUpRemoteArtifacts ? 'Cleaning up...' : 'Clean up remote artifacts' }}
                    </button>
                  </div>

                  <div
                    v-if="cleanupSummary"
                    data-testid="cleanup-summary"
                    class="mt-4 border border-fg2/15 bg-bg1/70 p-4"
                  >
                    <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Last cleanup result
                    </p>
                    <dl class="mt-4 grid gap-3 text-sm md:grid-cols-2 xl:grid-cols-5">
                      <div>
                        <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                          Closed tasks
                        </dt>
                        <dd class="mt-1 text-fg1">
                          {{ cleanupSummary.closedTasksCleaned }}
                        </dd>
                      </div>
                      <div>
                        <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                          Missing tasks
                        </dt>
                        <dd class="mt-1 text-fg1">
                          {{ cleanupSummary.missingTasksCleaned }}
                        </dd>
                      </div>
                      <div>
                        <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                          Local histories
                        </dt>
                        <dd class="mt-1 text-fg1">
                          {{ cleanupSummary.localDispatchHistoriesRemoved }}
                        </dd>
                      </div>
                      <div>
                        <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                          Worktrees
                        </dt>
                        <dd class="mt-1 text-fg1">
                          {{ cleanupSummary.remoteWorktreesRemoved }}
                        </dd>
                      </div>
                      <div>
                        <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                          Run dirs
                        </dt>
                        <dd class="mt-1 text-fg1">
                          {{ cleanupSummary.remoteRunDirectoriesRemoved }}
                        </dd>
                      </div>
                    </dl>
                  </div>
                </section>

                <section class="mt-4 border border-fg2/15 bg-bg0/60 p-4">
                  <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                    <div class="min-w-0">
                      <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                        Remote reset
                      </p>
                      <div class="mt-4 space-y-4 text-sm leading-7 text-fg1">
                        <p>
                          Remove the entire remote workspace managed by <code>track</code> and delete the remote projects registry, while keeping local tasks and local dispatch history intact.
                        </p>
                        <p>
                          Use this when the remote VM has drifted into an ambiguous state and you want the next dispatch to rebuild everything from local tracker data.
                        </p>
                        <p
                          v-if="activeRemoteWorkCount > 0"
                          class="text-yellow"
                        >
                          Stop active task runs and PR reviews before resetting the remote workspace.
                        </p>
                      </div>
                    </div>

                    <button
                      type="button"
                      data-testid="settings-reset-button"
                      class="border border-red/30 bg-red/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-red transition hover:bg-red/15 disabled:cursor-not-allowed disabled:opacity-60"
                      :disabled="resettingRemoteWorkspace || !remoteAgentSettings?.configured || activeRemoteWorkCount > 0"
                      @click="openRemoteResetConfirmation"
                    >
                      {{ resettingRemoteWorkspace ? 'Resetting...' : 'Reset remote workspace' }}
                    </button>
                  </div>

                  <div
                    v-if="resetSummary"
                    data-testid="reset-summary"
                    class="mt-4 border border-fg2/15 bg-bg1/70 p-4"
                  >
                    <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Last reset result
                    </p>
                    <dl class="mt-4 grid gap-3 text-sm md:grid-cols-2">
                      <div>
                        <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                          Workspace entries
                        </dt>
                        <dd class="mt-1 text-fg1">
                          {{ resetSummary.workspaceEntriesRemoved }}
                        </dd>
                      </div>
                      <div>
                        <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                          Registry
                        </dt>
                        <dd class="mt-1 text-fg1">
                          {{ resetSummary.registryRemoved ? 'Removed' : 'Already missing' }}
                        </dd>
                      </div>
                    </dl>
                  </div>
                </section>
              </section>
            </section>
          </template>
        </section>
      </div>
    </div>

    <div
      v-if="currentPage === 'tasks' && isTaskDrawerOpen && selectedTask"
      class="fixed inset-0 z-40 flex justify-end bg-bg0/70 backdrop-blur-[2px]"
      @click.self="closeTaskDrawer"
    >
      <aside
        data-testid="task-drawer"
        class="h-full w-full max-w-[1150px] overflow-y-auto border-l border-fg2/20 bg-bg1 shadow-panel"
      >
        <div class="space-y-5 p-5 sm:p-6">
          <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-5">
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-fg3">
                <button
                  v-if="selectedTaskProject"
                  type="button"
                  class="transition hover:text-fg0"
                  @click="selectProjectDetails(selectedTaskProject)"
                >
                  {{ selectedTask.project }}
                </button>
                <span v-else>{{ selectedTask.project }}</span>
                <span class="text-fg3/40">/</span>
                <span>{{ taskReference(selectedTask) }}</span>
              </div>

              <h2 class="mt-3 whitespace-pre-wrap font-display text-3xl leading-tight text-fg0 sm:text-4xl">
                {{ selectedTaskDescription?.title ?? taskTitle(selectedTask) }}
              </h2>

              <div class="mt-4 flex flex-wrap gap-2 text-[11px] font-semibold tracking-[0.08em]">
                <span class="border px-2 py-1" :class="priorityBadgeClass(selectedTask.priority)">
                  {{ selectedTask.priority }}
                </span>
                <span class="border px-2 py-1" :class="taskStatusBadgeClass(selectedTask.status)">
                  {{ selectedTask.status }}
                </span>
                <span class="border px-2 py-1" :class="dispatchBadgeClass(selectedTaskLatestDispatch)">
                  {{ dispatchStatusLabel(selectedTaskLatestDispatch) }}
                </span>
              </div>

              <p class="mt-4 text-sm leading-7 text-fg2">
                {{ formatTaskTimestamp(selectedTask) }}
              </p>
            </div>

            <button
              type="button"
              class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0 disabled:cursor-not-allowed disabled:opacity-60"
              :disabled="selectedTaskLifecycleMutation !== null"
              @click="closeTaskDrawer"
            >
              Close
            </button>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <button
              type="button"
              data-testid="drawer-primary-action"
              class="px-4 py-2.5 text-sm font-semibold tracking-[0.08em] transition disabled:cursor-not-allowed disabled:opacity-60"
              :class="drawerPrimaryActionClass(selectedTask, selectedTaskLatestDispatch)"
              :disabled="
                selectedTaskLifecycleMutation !== null ||
                dispatchingTaskId === selectedTask.id ||
                cancelingDispatchTaskId === selectedTask.id ||
                followingUpTaskId === selectedTask.id ||
                (selectedTask.status === 'open' &&
                  selectedTaskLatestDispatch?.status !== 'preparing' &&
                  selectedTaskLatestDispatch?.status !== 'running' &&
                  !selectedTaskCanContinue &&
                Boolean(selectedTaskDispatchDisabledReason))
              "
              @click="handlePrimaryAction"
            >
              {{ drawerPrimaryActionLabel(selectedTask, selectedTaskLatestDispatch) }}
            </button>

            <button
              type="button"
              class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0 disabled:cursor-not-allowed disabled:opacity-60"
              :disabled="selectedTaskLifecycleMutation !== null"
              @click="openTaskEditor(selectedTask)"
            >
              Edit
            </button>

            <button
              v-if="selectedTask.status === 'open'"
              type="button"
              class="border border-green/30 bg-green/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-green transition hover:bg-green/15 disabled:cursor-not-allowed disabled:opacity-60"
              :disabled="selectedTaskLifecycleMutation !== null"
              @click="updateTaskStatus(selectedTask, 'closed')"
            >
              {{ selectedTaskLifecycleMutation === 'closing' ? 'Closing...' : 'Close task' }}
            </button>

            <button
              v-if="selectedTaskLatestReusablePullRequest"
              type="button"
              class="border border-aqua/30 bg-aqua/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:cursor-not-allowed disabled:opacity-60"
              :disabled="selectedTaskLifecycleMutation !== null"
              @click="openExternal(selectedTaskLatestReusablePullRequest)"
            >
              View PR
            </button>

            <details class="relative" :class="selectedTaskLifecycleMutation !== null ? 'pointer-events-none opacity-60' : ''">
              <summary class="list-none border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0 cursor-pointer">
                More
              </summary>

              <div class="absolute right-0 z-10 mt-2 min-w-[210px] space-y-2 border border-fg2/20 bg-bg1 p-2 shadow-panel">
                <button
                  v-if="selectedTaskCanStartFresh"
                  type="button"
                  class="w-full border border-blue/25 bg-blue/8 px-3 py-2 text-left text-xs font-semibold tracking-[0.08em] text-blue transition hover:bg-blue/12 disabled:opacity-60"
                  :disabled="dispatchingTaskId === selectedTask.id"
                  @click="startRemoteRun(selectedTask)"
                >
                  {{
                    dispatchingTaskId === selectedTask.id
                      ? 'Starting...'
                      : `Start fresh via ${remoteAgentToolLabel(selectedTaskDispatchTool)}`
                  }}
                </button>

                <button
                  v-if="selectedTaskCanDiscardHistory"
                  type="button"
                  class="w-full border border-fg2/20 bg-bg0 px-3 py-2 text-left text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0 disabled:opacity-60"
                  :disabled="discardingDispatchTaskId === selectedTask.id"
                  @click="discardRunHistory(selectedTask)"
                >
                  {{ discardingDispatchTaskId === selectedTask.id ? 'Discarding...' : 'Discard history' }}
                </button>

                <button
                  type="button"
                  class="w-full border border-red/30 bg-red/10 px-3 py-2 text-left text-xs font-semibold tracking-[0.08em] text-red transition hover:bg-red/15"
                  @click="queueTaskDeletion(selectedTask)"
                >
                  {{ selectedTaskLifecycleMutation === 'deleting' ? 'Deleting...' : 'Delete' }}
                </button>
              </div>
            </details>

          </div>

          <section
            v-if="selectedTask.status === 'open'"
            class="border border-fg2/15 bg-bg0/60 p-4"
          >
            <div
              v-if="!selectedTaskPinnedTool"
              class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between"
            >
              <label class="block min-w-[220px] text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Dispatch via
                <select
                  v-model="selectedTaskStartTool"
                  data-testid="drawer-dispatch-tool"
                  class="mt-2 w-full border border-fg2/20 bg-bg1 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
                >
                  <option value="codex">
                    Codex
                  </option>
                  <option value="claude">
                    Claude
                  </option>
                </select>
              </label>
            </div>

            <p
              v-else
              data-testid="drawer-pinned-tool"
              class="max-w-2xl text-sm leading-7 text-fg2"
            >
              This task stays on
              <span class="text-fg0">{{ remoteAgentToolLabel(selectedTaskPinnedTool) }}</span>
              for future dispatches.
            </p>
          </section>

          <p
            v-if="selectedTaskLifecycleMutation"
            class="border border-blue/20 bg-blue/8 px-4 py-3 text-sm leading-6 text-blue"
          >
            {{ taskLifecycleProgressMessage(selectedTaskLifecycleMutation) }}
          </p>

          <p
            v-if="selectedTaskDispatchDisabledReason && selectedTask.status === 'open' && !selectedTaskCanContinue"
            class="border border-yellow/25 bg-yellow/8 px-4 py-3 text-sm leading-6 text-yellow"
          >
            {{ selectedTaskDispatchDisabledReason }}
          </p>

          <section class="border border-fg2/15 bg-bg0/60 p-4">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Summary
            </p>
            <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
              {{ selectedTaskDescription?.summaryMarkdown || selectedTask.description }}
            </div>
          </section>

          <section v-if="selectedTaskDescription?.originalNote" class="space-y-3">
            <details
              v-if="selectedTaskDescription?.originalNote"
              class="border border-fg2/15 bg-bg0/60 p-4"
            >
              <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Original note
              </summary>
              <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                {{ selectedTaskDescription.originalNote }}
              </div>
            </details>
          </section>

          <section class="border border-fg2/15 bg-bg0/60 p-4">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                  Run history
                </p>
                <p class="mt-2 text-sm text-fg2">
                  Every dispatch attempt is kept here so you can continue or start fresh with context.
                </p>
              </div>
              <span class="text-xs text-fg3">{{ selectedTaskRuns.length }}</span>
            </div>

            <div
              v-if="selectedTaskRuns.length === 0"
              class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
            >
              This task has no run history yet.
            </div>

            <div v-else class="mt-4 space-y-3">
              <article
                v-for="(run, index) in selectedTaskRuns"
                :key="run.dispatch.dispatchId"
                :data-dispatch-id="run.dispatch.dispatchId"
                data-testid="run-history-item"
                class="border border-fg2/15 bg-bg1/70 p-4"
              >
                <div class="flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                      <span
                        v-if="index === 0"
                        data-testid="run-latest-badge"
                        class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2"
                      >
                        Latest
                      </span>
                      <span class="border px-2 py-1" :class="dispatchBadgeClass(run.dispatch)">
                        {{ dispatchStatusLabel(run.dispatch) }}
                      </span>
                      <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                        via {{ remoteAgentToolLabel(run.dispatch.preferredTool) }}
                      </span>
                      <span class="text-fg3">Started {{ formatDateTime(run.dispatch.createdAt) }}</span>
                      <span v-if="run.dispatch.followUpRequest" class="text-fg3">• Follow-up</span>
                    </div>
                  </div>

                  <button
                    v-if="run.dispatch.pullRequestUrl"
                    type="button"
                    class="border border-aqua/30 bg-aqua/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                    @click="openExternal(run.dispatch.pullRequestUrl)"
                  >
                    View PR
                  </button>
                </div>

                <p class="mt-4 text-sm leading-7 text-fg1">
                  {{ dispatchSummary(run.dispatch) }}
                </p>

                <dl class="mt-4 grid gap-4 md:grid-cols-2 text-sm">
                  <div>
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Started
                    </dt>
                    <dd class="mt-1 text-fg1">
                      {{ formatDateTime(run.dispatch.createdAt) }}
                    </dd>
                  </div>
                  <div v-if="run.dispatch.finishedAt">
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Finished
                    </dt>
                    <dd class="mt-1 text-fg1">
                      {{ formatDateTime(run.dispatch.finishedAt) }}
                    </dd>
                  </div>
                  <div v-if="run.dispatch.branchName">
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Branch
                    </dt>
                    <dd class="mt-1 break-all text-fg1">
                      {{ run.dispatch.branchName }}
                    </dd>
                  </div>
                  <div v-if="run.dispatch.worktreePath">
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Worktree
                    </dt>
                    <dd class="mt-1 break-all text-fg1">
                      {{ run.dispatch.worktreePath }}
                    </dd>
                  </div>
                </dl>

                <details
                  v-if="run.dispatch.followUpRequest"
                  class="mt-4 border border-aqua/20 bg-aqua/6 p-4"
                >
                  <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-aqua">
                    Follow-up request
                  </summary>
                  <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                    {{ run.dispatch.followUpRequest }}
                  </div>
                </details>

                <details
                  v-if="run.dispatch.notes"
                  class="mt-4 border border-fg2/15 bg-bg0/70 p-4"
                >
                  <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Run notes
                  </summary>
                  <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                    {{ run.dispatch.notes }}
                  </div>
                </details>

                <details
                  v-if="run.dispatch.errorMessage"
                  class="mt-4 border border-red/20 bg-red/5 p-4"
                >
                  <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-red">
                    Error details
                  </summary>
                  <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-red">
                    {{ run.dispatch.errorMessage }}
                  </div>
                </details>
              </article>
            </div>
          </section>
        </div>
      </aside>
    </div>

    <div
      v-if="currentPage === 'reviews' && isReviewDrawerOpen && selectedReview"
      class="fixed inset-0 z-40 flex justify-end bg-bg0/70 backdrop-blur-[2px]"
      @click.self="closeReviewDrawer"
    >
      <aside
        data-testid="review-drawer"
        class="h-full w-full max-w-[980px] overflow-y-auto border-l border-fg2/20 bg-bg1 shadow-panel"
      >
        <div class="space-y-5 p-5 sm:p-6">
          <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-5">
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-fg3">
                <span>{{ selectedReview.repositoryFullName }}</span>
                <span class="text-fg3/40">/</span>
                <span>PR #{{ selectedReview.pullRequestNumber }}</span>
              </div>

              <h2 class="mt-3 whitespace-pre-wrap font-display text-3xl leading-tight text-fg0 sm:text-4xl">
                {{ selectedReview.pullRequestTitle }}
              </h2>

              <div class="mt-4 flex flex-wrap gap-2 text-[11px] font-semibold tracking-[0.08em]">
                <span class="border px-2 py-1" :class="dispatchBadgeClass(selectedReviewLatestRun)">
                  {{ dispatchStatusLabel(selectedReviewLatestRun) }}
                </span>
                <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                  via {{ remoteAgentToolLabel(selectedReview.preferredTool) }}
                </span>
                <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                  @{{ selectedReview.mainUser }}
                </span>
                <span
                  class="border px-2 py-1"
                  :class="selectedReviewLatestRun?.reviewSubmitted ? 'border-green/30 bg-green/10 text-green' : 'border-fg2/15 bg-bg0 text-fg2'"
                >
                  {{ selectedReviewLatestRun?.reviewSubmitted ? 'Review submitted' : 'Submission not confirmed' }}
                </span>
              </div>

              <p class="mt-4 text-sm leading-7 text-fg2">
                Created {{ formatDateTime(selectedReview.createdAt) }}
              </p>
            </div>

            <button
              type="button"
              class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0"
              @click="closeReviewDrawer"
            >
              Close
            </button>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <button
              v-if="selectedReviewCanCancel"
              type="button"
              class="border border-orange/30 bg-orange/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-orange transition hover:bg-orange/15 disabled:opacity-60"
              :disabled="cancelingReviewId === selectedReview.id"
              @click="cancelReviewRun(selectedReview)"
            >
              {{ cancelingReviewId === selectedReview.id ? 'Canceling...' : 'Cancel review run' }}
            </button>

            <button
              v-if="selectedReviewCanReReview"
              type="button"
              class="border border-aqua/30 bg-aqua/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:opacity-60"
              :disabled="followingUpReviewId === selectedReview.id"
              @click="openReviewFollowUpEditor(selectedReview)"
            >
              {{ followingUpReviewId === selectedReview.id ? 'Requesting...' : 'Request re-review' }}
            </button>

            <button
              type="button"
              class="border border-aqua/30 bg-aqua/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
              @click="openExternal(selectedReview.pullRequestUrl)"
            >
              View PR
            </button>

            <button
              v-if="selectedReviewLatestRun?.githubReviewUrl"
              type="button"
              class="border border-green/30 bg-green/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-green transition hover:bg-green/15"
              @click="openExternal(selectedReviewLatestRun.githubReviewUrl)"
            >
              View submitted review
            </button>

            <button
              type="button"
              class="border border-red/30 bg-red/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-red transition hover:bg-red/15 disabled:opacity-60"
              :disabled="saving"
              @click="queueReviewDeletion(selectedReview)"
            >
              Delete review
            </button>
          </div>

          <section class="border border-fg2/15 bg-bg0/60 p-4">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Latest status
            </p>
            <p class="mt-4 text-sm leading-7 text-fg1">
              {{ dispatchSummary(selectedReviewLatestRun, 'review') }}
            </p>
            <p class="mt-4 text-xs leading-6 text-fg3">
              The actual review discussion lives on GitHub, including any inline comments the agent submitted.
            </p>
            <dl class="mt-4 grid gap-4 text-sm md:grid-cols-2 xl:grid-cols-3">
              <div>
                <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Pull request
                </dt>
                <dd class="mt-1 break-all text-fg1">
                  {{ selectedReview.pullRequestUrl }}
                </dd>
              </div>
              <div>
                <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Base branch
                </dt>
                <dd class="mt-1 text-fg1">
                  {{ selectedReview.baseBranch }}
                </dd>
              </div>
              <div>
                <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Workspace key
                </dt>
                <dd class="mt-1 break-all text-fg1">
                  {{ selectedReview.workspaceKey }}
                </dd>
              </div>
              <div>
                <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Review tool
                </dt>
                <dd class="mt-1 text-fg1">
                  {{ remoteAgentToolLabel(selectedReview.preferredTool) }}
                </dd>
              </div>
              <div v-if="selectedReviewLatestRun?.targetHeadOid">
                <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Pinned commit
                </dt>
                <dd class="mt-1 break-all text-fg1">
                  {{ selectedReviewLatestRun.targetHeadOid }}
                </dd>
              </div>
              <div v-if="selectedReviewLatestRun?.githubReviewUrl">
                <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Submitted review
                </dt>
                <dd class="mt-1 break-all text-fg1">
                  {{ selectedReviewLatestRun.githubReviewUrl }}
                </dd>
              </div>
            </dl>
          </section>

          <section class="grid gap-4 xl:grid-cols-2">
            <section class="border border-fg2/15 bg-bg0/60 p-4">
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Default review prompt
              </p>
              <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                {{ selectedReview.defaultReviewPrompt || 'No default review prompt was saved for this review.' }}
              </div>
            </section>

            <section class="border border-fg2/15 bg-bg0/60 p-4">
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Extra instructions
              </p>
              <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                {{ selectedReview.extraInstructions || 'No extra instructions were provided for this review.' }}
              </div>
            </section>
          </section>

          <section class="border border-fg2/15 bg-bg0/60 p-4">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                  Review run history
                </p>
                <p class="mt-2 text-sm text-fg2">
                  Each re-review adds another run here so you can compare requests, commits, and outcomes over time.
                </p>
              </div>
              <span class="text-xs text-fg3">{{ selectedReviewRuns.length }}</span>
            </div>

            <div
              v-if="selectedReviewRuns.length === 0"
              class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
            >
              This review has no run history yet.
            </div>

            <div v-else class="mt-4 space-y-3">
              <article
                v-for="(run, index) in selectedReviewRuns"
                :key="run.dispatchId"
                class="border border-fg2/15 bg-bg1/70 p-4"
              >
                <div class="flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                      <span
                        v-if="index === 0"
                        class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2"
                      >
                        Latest
                      </span>
                      <span class="border px-2 py-1" :class="dispatchBadgeClass(run)">
                        {{ dispatchStatusLabel(run) }}
                      </span>
                      <span class="text-fg3">Started {{ formatDateTime(run.createdAt) }}</span>
                      <span v-if="run.followUpRequest" class="text-fg3">• Re-review</span>
                    </div>
                  </div>

                  <button
                    v-if="run.githubReviewUrl"
                    type="button"
                    class="border border-green/30 bg-green/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-green transition hover:bg-green/15"
                    @click="openExternal(run.githubReviewUrl)"
                  >
                    View review
                  </button>
                </div>

                <p class="mt-4 text-sm leading-7 text-fg1">
                  {{ dispatchSummary(run, 'review') }}
                </p>

                <dl class="mt-4 grid gap-4 text-sm md:grid-cols-2 xl:grid-cols-3">
                  <div v-if="run.finishedAt">
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Finished
                    </dt>
                    <dd class="mt-1 text-fg1">
                      {{ formatDateTime(run.finishedAt) }}
                    </dd>
                  </div>
                  <div v-if="run.branchName">
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Branch
                    </dt>
                    <dd class="mt-1 break-all text-fg1">
                      {{ run.branchName }}
                    </dd>
                  </div>
                  <div v-if="run.worktreePath">
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Worktree
                    </dt>
                    <dd class="mt-1 break-all text-fg1">
                      {{ run.worktreePath }}
                    </dd>
                  </div>
                  <div v-if="run.targetHeadOid">
                    <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                      Pinned commit
                    </dt>
                    <dd class="mt-1 break-all text-fg1">
                      {{ run.targetHeadOid }}
                    </dd>
                  </div>
                </dl>

                <details
                  v-if="run.followUpRequest"
                  class="mt-4 border border-aqua/20 bg-aqua/6 p-4"
                >
                  <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-aqua">
                    Re-review request
                  </summary>
                  <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                    {{ run.followUpRequest }}
                  </div>
                </details>

                <details
                  v-if="run.notes"
                  class="mt-4 border border-fg2/15 bg-bg0/70 p-4"
                >
                  <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Run notes
                  </summary>
                  <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                    {{ run.notes }}
                  </div>
                </details>

                <details
                  v-if="run.errorMessage"
                  class="mt-4 border border-red/20 bg-red/5 p-4"
                >
                  <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-red">
                    Error details
                  </summary>
                  <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-red">
                    {{ run.errorMessage }}
                  </div>
                </details>
              </article>
            </div>
          </section>
        </div>
      </aside>
    </div>

    <TaskEditorModal
      :busy="saving"
      :default-project="defaultCreateProject"
      :mode="creatingTask ? 'create' : 'edit'"
      :open="creatingTask || editingTask !== null"
      :projects="availableProjects"
      :task="editingTask"
      @cancel="closeTaskEditor"
      @save="creatingTask ? createTaskFromWeb($event) : saveTaskEdits($event)"
    />

    <ReviewRequestModal
      :busy="saving"
      :default-preferred-tool="defaultRemoteAgentPreferredTool"
      :main-user="remoteAgentSettings?.reviewFollowUp?.mainUser"
      :open="creatingReview"
      @cancel="closeReviewEditor"
      @save="createReviewFromWeb"
    />

    <ReviewFollowUpModal
      :busy="followingUpReviewId !== null"
      :open="followingUpReview !== null"
      :review="followingUpReview"
      @cancel="closeReviewFollowUpEditor"
      @save="submitReviewFollowUp"
    />

    <ProjectMetadataModal
      :busy="saving"
      :open="editingProject !== null"
      :project="editingProject"
      @cancel="closeProjectEditor"
      @save="saveProjectEdits"
    />

    <RemoteAgentSetupModal
      :busy="saving"
      :open="editingRemoteAgentSetup"
      :required-for-dispatch="taskPendingRunnerSetup !== null"
      :settings="remoteAgentSettings"
      @cancel="closeRunnerSetup"
      @save="saveRemoteAgentSetup"
    />

    <FollowUpModal
      :busy="followingUpTaskId !== null"
      :dispatch="followingUpDispatch"
      :open="followingUpTask !== null"
      :task="followingUpTask"
      @cancel="closeFollowUpEditor"
      @save="submitFollowUp"
    />

    <ConfirmDialog
      :busy="saving"
      confirm-busy-label="Deleting..."
      confirm-label="Delete forever"
      confirm-variant="danger"
      :description="taskPendingDeletion ? `Delete ${taskTitle(taskPendingDeletion)} permanently? This cannot be undone.` : ''"
      eyebrow="Destructive action"
      :open="taskPendingDeletion !== null"
      title="Delete task"
      @cancel="clearPendingDeletion"
      @confirm="confirmDelete"
    />

    <ConfirmDialog
      :busy="saving"
      confirm-busy-label="Deleting..."
      confirm-label="Delete review"
      confirm-variant="danger"
      :description="reviewPendingDeletion ? `Delete the saved review for ${reviewPendingDeletion.repositoryFullName} PR #${reviewPendingDeletion.pullRequestNumber}? This removes local history and remote review artifacts.` : ''"
      eyebrow="Destructive action"
      :open="reviewPendingDeletion !== null"
      title="Delete PR review"
      @cancel="clearPendingReviewDeletion"
      @confirm="confirmReviewDelete"
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
  </main>
</template>
