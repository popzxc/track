<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import {
  ApiClientError,
  cancelDispatch,
  cleanupRemoteAgentArtifacts,
  createTask,
  deleteTask,
  discardDispatch,
  dispatchTask,
  fetchProjects,
  fetchRemoteAgentSettings,
  fetchRuns,
  fetchTaskChangeVersion,
  fetchTasks,
  followUpTask,
  updateProject,
  updateRemoteAgentSettings,
  updateTask,
} from './api/client'
import ConfirmDialog from './components/ConfirmDialog.vue'
import FollowUpModal from './components/FollowUpModal.vue'
import ProjectMetadataModal from './components/ProjectMetadataModal.vue'
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
  latestDispatchByTaskId as buildLatestDispatchByTaskId,
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
  ProjectInfo,
  ProjectMetadataUpdateInput,
  RemoteCleanupSummary,
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
  RunRecord,
  Task,
  TaskCreateInput,
  TaskDispatch,
  TaskFollowUpInput,
} from './types/task'

type AppPage = 'tasks' | 'runs' | 'projects' | 'settings'

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
const projects = ref<ProjectInfo[]>([])
const taskProjectOptions = ref<ProjectInfo[]>([])
const runs = ref<RunRecord[]>([])
const remoteAgentSettings = ref<RemoteAgentSettings | null>(null)
const showClosed = ref(false)
const selectedProjectFilter = ref('')
const selectedTaskId = ref<string | null>(null)
const pendingSelectedTaskId = ref<string | null>(null)
const selectedProjectDetailsId = ref<string | null>(null)
const isTaskDrawerOpen = ref(false)
const taskChangeVersion = ref<number | null>(null)
const loading = ref(true)
const refreshing = ref(false)
const saving = ref(false)
const dispatchingTaskId = ref<string | null>(null)
const cancelingDispatchTaskId = ref<string | null>(null)
const discardingDispatchTaskId = ref<string | null>(null)
const followingUpTaskId = ref<string | null>(null)
const errorMessage = ref('')

const creatingTask = ref(false)
const editingTask = ref<Task | null>(null)
const editingProject = ref<ProjectInfo | null>(null)
const editingRemoteAgentSetup = ref(false)
const followingUpTask = ref<Task | null>(null)
const taskPendingDeletion = ref<Task | null>(null)
const taskPendingRunnerSetup = ref<Task | null>(null)
const cleanupPendingConfirmation = ref(false)
const cleaningUpRemoteArtifacts = ref(false)
const cleanupSummary = ref<RemoteCleanupSummary | null>(null)

let taskChangePollTimer: number | null = null
let taskChangePollInFlight = false
let runPollTimer: number | null = null
let runPollInFlight = false

// =============================================================================
// Derived State
// =============================================================================
//
// The redesign keeps "tasks", "runs", and "project metadata" as separate
// concepts. The queue stays quiet, while richer context lives in the drawer and
// the dedicated Runs / Projects pages.
const visibleTaskCount = computed(() => tasks.value.length)
const totalProjectCount = computed(() => availableProjects.value.length)
const runnerSetupReady = computed(() =>
  Boolean(remoteAgentSettings.value?.configured && remoteAgentSettings.value.shellPrelude?.trim()),
)

const availableProjects = computed(() => mergeProjects(projects.value, taskProjectOptions.value))

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

const latestDispatchByTaskId = computed<Record<string, TaskDispatch>>(() => {
  return buildLatestDispatchByTaskId(runs.value)
})

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

const selectedTaskRuns = computed(() =>
  runs.value
    .filter((run) => run.task.id === selectedTask.value?.id)
    .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt)),
)

const selectedTaskDescription = computed<ParsedTaskDescription | null>(() =>
  selectedTask.value ? parseTaskDescription(selectedTask.value.description) : null,
)

const selectedTaskLatestReusablePullRequest = computed(() =>
  selectedTaskRuns.value.find((run) => Boolean(run.dispatch.pullRequestUrl))?.dispatch.pullRequestUrl ?? null,
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

const activeRuns = computed(() =>
  runs.value
    .filter((run) => run.dispatch.status === 'preparing' || run.dispatch.status === 'running')
    .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt)),
)

const recentRuns = computed(() =>
  runs.value
    .slice()
    .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
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
      return 'Reopen task'
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

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
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

function removeTaskRuns(taskId: string) {
  runs.value = runs.value.filter((run) => run.task.id !== taskId)
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
    path: '',
    aliases: [],
    metadata: undefined,
  }))
}

async function loadRuns() {
  runs.value = await fetchRuns(200)
}

async function syncTaskChangeVersion() {
  taskChangeVersion.value = await fetchTaskChangeVersion()
}

async function refreshAll() {
  errorMessage.value = ''
  refreshing.value = true

  try {
    await Promise.all([
      loadProjects(),
      loadTasks(),
      loadRuns(),
      syncTaskChangeVersion(),
      loadRemoteAgentSettings().catch(() => {
        // Runner setup is useful context, but the rest of the app should still
        // render if that endpoint is temporarily unavailable.
      }),
    ])
  } catch (error) {
    setFriendlyError(error)
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

  try {
    await updateTask(task.id, { status })
    await refreshAll()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
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
        void startRemoteRun(queuedTask)
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

async function confirmDelete() {
  if (!taskPendingDeletion.value) {
    return
  }

  saving.value = true
  errorMessage.value = ''

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
  }
}

async function startRemoteRun(task: Task) {
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
      'Remote dispatch is not configured yet. Re-run `track` locally and add a remote agent host plus SSH key.'
    currentPage.value = 'settings'
    return
  }

  if (remoteAgentSettings.value && !runnerSetupReady.value) {
    taskPendingRunnerSetup.value = task
    editingRemoteAgentSetup.value = true
    currentPage.value = 'settings'
    return
  }

  dispatchingTaskId.value = task.id
  errorMessage.value = ''

  try {
    const dispatch = await dispatchTask(task.id)
    upsertRunRecord(task, dispatch)
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
  } catch (error) {
    await loadRuns().catch(() => undefined)
    setFriendlyError(error)
  } finally {
    cancelingDispatchTaskId.value = null
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

function openRemoteCleanupConfirmation() {
  cleanupPendingConfirmation.value = true
}

function clearPendingRemoteCleanup() {
  cleanupPendingConfirmation.value = false
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
    discardingDispatchTaskId.value !== null ||
    followingUpTaskId.value !== null
  ) {
    return
  }

  if (activeRuns.value.length === 0) {
    return
  }

  runPollInFlight = true

  try {
    await loadRuns()
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

  void loadTasks().catch(setFriendlyError)
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
  }
})

watch(selectedTask, (task) => {
  if (!task) {
    isTaskDrawerOpen.value = false
  }
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
                currentPage === 'runs'
                  ? 'border-aqua/35 bg-aqua/10 text-aqua'
                  : 'border-fg2/20 bg-bg0 text-fg1 hover:border-fg1/35 hover:text-fg0'
              "
              @click="currentPage = 'runs'"
            >
              <span>Runs</span>
              <span class="text-xs text-fg3">{{ activeRuns.length }}</span>
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
              <span>Active runs</span>
              <span>{{ activeRuns.length }}</span>
            </div>
            <div class="mt-2 flex items-center justify-between">
              <span>Visible tasks</span>
              <span>{{ visibleTaskCount }}</span>
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
            <section v-if="currentPage === 'tasks'" class="space-y-4">
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
                      Running now
                    </p>
                    <p class="mt-2 text-sm text-fg2">
                      Live remote work that is still preparing or actively running.
                    </p>
                  </div>
                  <span class="text-xs text-fg3">{{ activeRuns.length }}</span>
                </div>

                <div
                  v-if="activeRuns.length === 0"
                  class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
                >
                  Nothing is running at the moment.
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
                      Projects appear after the CLI or web UI creates tasks under the track data directory.
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
                        {{ project.path || 'Tracked only in the data directory' }}
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
                            {{ selectedProjectDetails.path || 'Tracked through local task data only.' }}
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
                  Remote runner configuration
                </p>
              </div>

              <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
                <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                  <div class="min-w-0">
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

                    <dl class="mt-5 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
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
                    </dl>
                  </div>

                  <button
                    type="button"
                    class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                    @click="openRunnerSetup"
                  >
                    Edit runner setup
                  </button>
                </div>

                <div class="mt-6 grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(320px,420px)]">
                  <section class="border border-fg2/15 bg-bg0/60 p-4">
                    <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                      Why this exists
                    </p>
                    <div class="mt-4 space-y-4 text-sm leading-7 text-fg1">
                      <p>
                        The remote runner uses non-interactive SSH sessions, so it cannot rely on the environment tweaks that usually live in your interactive shell.
                      </p>
                      <p>
                        Keep the shell prelude focused on PATH and toolchain setup. The backend reuses it before every remote command so dispatches stay predictable.
                      </p>
                    </div>
                  </section>

                  <section class="border border-fg2/15 bg-bg0/60 p-4">
                    <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                      Current shell prelude
                    </p>
                    <pre class="mt-4 overflow-x-auto whitespace-pre-wrap text-sm leading-7 text-fg1">{{ remoteAgentSettings?.shellPrelude || 'No shell prelude has been saved yet.' }}</pre>
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
              class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0"
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
              class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
              @click="openTaskEditor(selectedTask)"
            >
              Edit
            </button>

            <button
              v-if="selectedTask.status === 'open'"
              type="button"
              class="border border-green/30 bg-green/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-green transition hover:bg-green/15"
              @click="updateTaskStatus(selectedTask, 'closed')"
            >
              Close task
            </button>

            <button
              v-if="selectedTaskLatestReusablePullRequest"
              type="button"
              class="border border-aqua/30 bg-aqua/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
              @click="openExternal(selectedTaskLatestReusablePullRequest)"
            >
              View PR
            </button>

            <details class="relative">
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
                  Start fresh
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
                  Delete
                </button>
              </div>
            </details>

          </div>

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
  </main>
</template>
