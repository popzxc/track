<script setup lang="ts">
import { computed, ref } from 'vue'

import { ApiClientError } from './api/client'
import ConfirmDialog from './components/ConfirmDialog.vue'
import FollowUpModal from './components/FollowUpModal.vue'
import MigrationStatePanel from './components/MigrationStatePanel.vue'
import ProjectsPage from './components/ProjectsPage.vue'
import ProjectMetadataModal from './components/ProjectMetadataModal.vue'
import ReviewDrawer from './components/ReviewDrawer.vue'
import ReviewsPage from './components/ReviewsPage.vue'
import ReviewFollowUpModal from './components/ReviewFollowUpModal.vue'
import ReviewRequestModal from './components/ReviewRequestModal.vue'
import RemoteAgentSetupModal from './components/RemoteAgentSetupModal.vue'
import RunsPage from './components/RunsPage.vue'
import SettingsPage from './components/SettingsPage.vue'
import TaskDrawer from './components/TaskDrawer.vue'
import TaskEditorModal from './components/TaskEditorModal.vue'
import TasksPage from './components/TasksPage.vue'
import { useAppDataLoader } from './composables/useAppDataLoader'
import { useBackgroundSync } from './composables/useBackgroundSync'
import { useReviewMutations } from './composables/useReviewMutations'
import { useProjectViewState } from './composables/useProjectViewState'
import { useReviewViewState } from './composables/useReviewViewState'
import { useRunState } from './composables/useRunState'
import { useSettingsMutations } from './composables/useSettingsMutations'
import { useShellOverlays } from './composables/useShellOverlays'
import { useTaskMutations, type PendingRunnerSetupRequest } from './composables/useTaskMutations'
import { useTaskViewState } from './composables/useTaskViewState'
import {
  drawerPrimaryAction,
  groupTasksByProject,
  mergeProjects,
} from './features/tasks/presentation'
import {
  taskTitle,
} from './features/tasks/description'
import type {
  MigrationImportSummary,
  MigrationStatus,
  ProjectInfo,
  RemoteCleanupSummary,
  RemoteResetSummary,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
  TaskDispatch,
} from './types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'
type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

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
const latestDispatchByTaskId = computed<Record<string, TaskDispatch>>(
  () => latestTaskDispatchesByTaskId.value,
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
const migrationRequired = computed(() => Boolean(migrationStatus.value?.requiresMigration))
const migrationGateActive = computed(() => Boolean(migrationRequired.value && migrationStatus.value))

const {
  closeTaskDrawer,
  isTaskDrawerOpen,
  openTaskFromRun,
  pendingSelectedTaskId,
  selectTask,
  selectedProjectFilter,
  selectedTask,
  selectedTaskCanContinue,
  selectedTaskCanDiscardHistory,
  selectedTaskCanStartFresh,
  selectedTaskDispatchDisabledReason,
  selectedTaskDispatchTool,
  selectedTaskId,
  selectedTaskLatestDispatch,
  selectedTaskLatestReusablePullRequest,
  selectedTaskLifecycleMessage,
  selectedTaskLifecycleMutation,
  selectedTaskPinnedTool,
  selectedTaskPrimaryActionDisabled,
  selectedTaskProject,
  selectedTaskStartTool,
  showClosed,
} = useTaskViewState({
  availableProjects,
  cancelingDispatchTaskId,
  currentPage,
  defaultRemoteAgentPreferredTool,
  dispatchingTaskId,
  followingUpTaskId,
  latestDispatchByTaskId,
  remoteAgentSettings,
  selectedTaskRuns,
  taskLifecycleMutation,
  taskLifecycleMutationTaskId,
  tasks,
})

const {
  defaultCreateProject,
  selectProjectDetails,
  selectedProjectDetails,
  selectedProjectDetailsId,
} = useProjectViewState({
  availableProjects,
  closeTaskDrawer,
  currentPage,
  selectedProjectFilter,
})

const {
  closeReviewDrawer,
  isReviewDrawerOpen,
  selectReview,
  selectedReview,
  selectedReviewCanCancel,
  selectedReviewCanReReview,
  selectedReviewId,
  selectedReviewLatestRun,
} = useReviewViewState({
  currentPage,
  followingUpReview,
  reviews,
  selectedReviewRuns,
})

const {
  activeReviewRuns,
  activeRuns,
  loadLatestDispatchesForVisibleTasks,
  loadReviews,
  loadRuns,
  loadSelectedReviewRunHistory,
  loadSelectedTaskRunHistory,
  recentReviewRuns,
  recentRuns,
  removeReview,
  removeTaskRuns,
  replaceSelectedReviewRuns,
  upsertLatestReviewRun,
  upsertLatestTaskDispatch,
  upsertReviewSummary,
  upsertRunRecord,
  upsertSelectedReviewRun,
  upsertSelectedTaskRun,
} = useRunState({
  closeReviewDrawer,
  isReviewDrawerOpen,
  isTaskDrawerOpen,
  latestTaskDispatchesByTaskId,
  reviews,
  runs,
  selectedReview,
  selectedReviewId,
  selectedReviewRuns,
  selectedTask,
  selectedTaskId,
  selectedTaskRuns,
  tasks,
})

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

const activeRemoteWorkCount = computed(() => activeRuns.value.length + activeReviewRuns.value.length)

const followingUpDispatch = computed(() =>
  followingUpTask.value ? latestDispatchByTaskId.value[followingUpTask.value.id] ?? undefined : undefined,
)

const shellPreludeHelpText = 'The remote runner uses non-interactive SSH sessions, so it cannot rely on the environment tweaks that usually live in your interactive shell.\n\nKeep the shell prelude focused on PATH and toolchain setup. The backend reuses it before every remote command so dispatches stay predictable.'

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

function openSelectedTaskProjectDetails() {
  if (!selectedTaskProject.value) {
    return
  }

  selectProjectDetails(selectedTaskProject.value)
}

let syncTaskChangeVersion = async () => undefined

const {
  loadRemoteAgentSettings,
  loadTasks,
  refreshAll,
} = useAppDataLoader({
  errorMessage,
  latestTaskDispatchesByTaskId,
  loading,
  loadLatestDispatchesForVisibleTasks,
  loadReviews,
  loadRuns,
  loadSelectedReviewRunHistory,
  loadSelectedTaskRunHistory,
  migrationStatus,
  projects,
  refreshing,
  remoteAgentSettings,
  reviews,
  runs,
  selectedProjectFilter,
  selectedReviewRuns,
  selectedTaskRuns,
  setFriendlyError,
  showClosed,
  syncTaskChangeVersion: () => syncTaskChangeVersion(),
  taskProjectOptions,
  tasks,
})

const {
  cancelRemoteRun,
  confirmDelete,
  createTaskFromWeb,
  discardRunHistory,
  handlePrimaryAction,
  saveTaskEdits,
  startRemoteRun,
  submitFollowUp,
  updateTaskStatus,
} = useTaskMutations({
  cancelingDispatchTaskId,
  closeTaskDrawer,
  creatingTask,
  currentPage,
  discardingDispatchTaskId,
  dispatchingTaskId,
  editingRemoteAgentSetup,
  editingTask,
  errorMessage,
  followingUpTask,
  followingUpTaskId,
  isTaskDrawerOpen,
  loadRemoteAgentSettings,
  loadRuns,
  pendingSelectedTaskId,
  refreshAll,
  remoteAgentSettings,
  removeTaskRuns,
  runnerSetupReady,
  saving,
  selectedProjectFilter,
  selectedTask,
  selectedTaskCanContinue,
  selectedTaskDispatchTool,
  selectedTaskId,
  selectedTaskLatestDispatch,
  setFriendlyError,
  showClosed,
  taskLifecycleMutation,
  taskLifecycleMutationTaskId,
  taskPendingDeletion,
  taskPendingRunnerSetup,
  upsertLatestTaskDispatch,
  upsertRunRecord,
  upsertSelectedTaskRun,
})

const {
  cancelReviewRun,
  confirmReviewDelete,
  createReviewFromWeb,
  submitReviewFollowUp,
} = useReviewMutations({
  cancelingReviewId,
  creatingReview,
  currentPage,
  errorMessage,
  followingUpReview,
  followingUpReviewId,
  refreshAll,
  removeReview,
  replaceSelectedReviewRuns,
  reviewPendingDeletion,
  saving,
  selectReview,
  setFriendlyError,
  upsertLatestReviewRun,
  upsertReviewSummary,
  upsertSelectedReviewRun,
})

const {
  confirmRemoteCleanup,
  confirmRemoteReset,
  importLegacyTrackerData,
  saveProjectEdits,
  saveRemoteAgentSetup,
} = useSettingsMutations({
  cleaningUpRemoteArtifacts,
  cleanupPendingConfirmation,
  cleanupSummary,
  editingProject,
  editingRemoteAgentSetup,
  errorMessage,
  migrationImportPending,
  migrationImportSummary,
  migrationStatus,
  refreshAll,
  remoteAgentSettings,
  resetPendingConfirmation,
  resetSummary,
  resettingRemoteWorkspace,
  resumeQueuedTaskDispatch(task, preferredTool) {
    void startRemoteRun(task, preferredTool)
  },
  saving,
  setFriendlyError,
  taskPendingRunnerSetup,
})

const {
  clearPendingDeletion,
  clearPendingRemoteCleanup,
  clearPendingRemoteReset,
  clearPendingReviewDeletion,
  closeFollowUpEditor,
  closeProjectEditor,
  closeReviewEditor,
  closeReviewFollowUpEditor,
  closeRunnerSetup,
  closeTaskEditor,
  openNewReviewEditor,
  openNewTaskEditor,
  openProjectEditor,
  openRemoteCleanupConfirmation,
  openRemoteResetConfirmation,
  openReviewFollowUpEditor,
  openRunnerSetup,
  openTaskEditor,
  queueReviewDeletion,
  queueTaskDeletion,
} = useShellOverlays({
  cleanupPendingConfirmation,
  creatingReview,
  creatingTask,
  editingProject,
  editingRemoteAgentSetup,
  editingTask,
  followingUpReview,
  followingUpTask,
  resetPendingConfirmation,
  reviewPendingDeletion,
  selectedProjectDetails,
  selectedReview,
  taskPendingDeletion,
  taskPendingRunnerSetup,
})

const backgroundSync = useBackgroundSync({
  activeReviewRuns,
  activeRuns,
  cancelingDispatchTaskId,
  cancelingReviewId,
  dispatchingTaskId,
  discardingDispatchTaskId,
  followingUpTaskId,
  isReviewDrawerOpen,
  isTaskDrawerOpen,
  loading,
  loadLatestDispatchesForVisibleTasks,
  loadReviews,
  loadRuns,
  loadSelectedReviewRunHistory,
  loadSelectedTaskRunHistory,
  loadTasks,
  refreshAll,
  refreshing,
  saving,
  selectedProjectFilter,
  selectedReview,
  selectedReviewRuns,
  selectedTask,
  selectedTaskRuns,
  setFriendlyError,
  showClosed,
})
syncTaskChangeVersion = backgroundSync.syncTaskChangeVersion
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
            <MigrationStatePanel
              :migration-import-pending="migrationImportPending"
              :migration-import-summary="migrationImportSummary"
              :migration-required="migrationRequired"
              :migration-status="migrationStatus"
              @request-import-legacy-data="importLegacyTrackerData"
            />

            <TasksPage
              v-if="!migrationGateActive && currentPage === 'tasks'"
              :active-task-id="selectedTask?.id ?? null"
              :drawer-open="isTaskDrawerOpen"
              :latest-dispatch-by-task-id="latestTaskDispatchesByTaskId"
              :projects="availableProjects"
              :selected-project-filter="selectedProjectFilter"
              :show-closed="showClosed"
              :task-count="tasks.length"
              :task-groups="taskGroups"
              @request-create-task="openNewTaskEditor"
              @request-select-task="selectTask"
              @update:selected-project-filter="selectedProjectFilter = $event"
              @update:show-closed="showClosed = $event"
            />

            <ReviewsPage
              v-else-if="!migrationGateActive && currentPage === 'reviews'"
              :can-request-review="canRequestReview"
              :review-request-disabled-reason="reviewRequestDisabledReason"
              :reviews="reviews"
              @request-create-review="openNewReviewEditor"
              @request-open-settings="currentPage = 'settings'"
              @request-select-review="selectReview"
            />

            <RunsPage
              v-else-if="!migrationGateActive && currentPage === 'runs'"
              :active-review-runs="activeReviewRuns"
              :active-runs="activeRuns"
              :recent-review-runs="recentReviewRuns"
              :recent-runs="recentRuns"
              @request-open-review="selectReview"
              @request-open-task-run="openTaskFromRun"
              @request-open-url="openExternal"
            />

            <ProjectsPage
              v-else-if="!migrationGateActive && currentPage === 'projects'"
              :projects="availableProjects"
              :selected-project-details="selectedProjectDetails"
              :selected-project-id="selectedProjectDetailsId"
              @request-edit-project="openProjectEditor"
              @request-select-project="selectedProjectDetailsId = $event"
            />

            <SettingsPage
              v-else-if="!migrationGateActive"
              :active-remote-work-count="activeRemoteWorkCount"
              :cleaning-up-remote-artifacts="cleaningUpRemoteArtifacts"
              :cleanup-summary="cleanupSummary"
              :remote-agent-settings="remoteAgentSettings"
              :reset-summary="resetSummary"
              :resetting-remote-workspace="resettingRemoteWorkspace"
              :runner-setup-ready="runnerSetupReady"
              :shell-prelude-help-text="shellPreludeHelpText"
              @request-open-cleanup="openRemoteCleanupConfirmation"
              @request-open-reset="openRemoteResetConfirmation"
              @request-open-runner-setup="openRunnerSetup"
            />
          </template>
        </section>
      </div>
    </div>

    <TaskDrawer
      v-if="currentPage === 'tasks' && isTaskDrawerOpen && selectedTask"
      :can-continue="selectedTaskCanContinue"
      :can-discard-history="selectedTaskCanDiscardHistory"
      :can-start-fresh="selectedTaskCanStartFresh"
      :dispatch-disabled-reason="selectedTaskDispatchDisabledReason"
      :is-discarding-history="discardingDispatchTaskId === selectedTask.id"
      :is-dispatching="dispatchingTaskId === selectedTask.id"
      :latest-dispatch="selectedTaskLatestDispatch"
      :latest-reusable-pull-request="selectedTaskLatestReusablePullRequest"
      :lifecycle-mutation="selectedTaskLifecycleMutation"
      :lifecycle-progress-message="selectedTaskLifecycleMessage"
      :pinned-tool="selectedTaskPinnedTool"
      :primary-action-class="drawerPrimaryActionClass(selectedTask, selectedTaskLatestDispatch)"
      :primary-action-disabled="selectedTaskPrimaryActionDisabled"
      :primary-action-label="drawerPrimaryActionLabel(selectedTask, selectedTaskLatestDispatch)"
      :start-tool="selectedTaskDispatchTool"
      :task="selectedTask"
      :task-project="selectedTaskProject"
      :task-runs="selectedTaskRuns"
      @close="closeTaskDrawer"
      @request-close-task="updateTaskStatus(selectedTask, 'closed')"
      @request-delete-task="queueTaskDeletion(selectedTask)"
      @request-discard-history="discardRunHistory(selectedTask)"
      @request-edit-task="openTaskEditor(selectedTask)"
      @request-open-project="openSelectedTaskProjectDetails"
      @request-open-url="openExternal"
      @request-primary-action="handlePrimaryAction"
      @request-start-fresh="startRemoteRun(selectedTask)"
      @update:start-tool="selectedTaskStartTool = $event"
    />

    <ReviewDrawer
      v-if="currentPage === 'reviews' && isReviewDrawerOpen && selectedReview"
      :can-cancel="selectedReviewCanCancel"
      :can-re-review="selectedReviewCanReReview"
      :canceling-review-id="cancelingReviewId"
      :following-up-review-id="followingUpReviewId"
      :latest-run="selectedReviewLatestRun"
      :review="selectedReview"
      :review-runs="selectedReviewRuns"
      :saving="saving"
      @close="closeReviewDrawer"
      @request-cancel-review-run="cancelReviewRun"
      @request-delete-review="queueReviewDeletion"
      @request-open-url="openExternal"
      @request-rereview="openReviewFollowUpEditor"
    />

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
