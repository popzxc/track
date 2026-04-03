<script setup lang="ts">
import { computed, ref } from 'vue'

import { ApiClientError } from './api/client'
import MigrationStatePanel from './components/MigrationStatePanel.vue'
import ProjectsPage from './components/ProjectsPage.vue'
import ReviewsPage from './components/ReviewsPage.vue'
import ShellOverlayMount from './components/ShellOverlayMount.vue'
import ShellSidebar from './components/ShellSidebar.vue'
import RunsPage from './components/RunsPage.vue'
import SettingsPage from './components/SettingsPage.vue'
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
  groupTasksByProject,
  mergeProjects,
} from './features/tasks/presentation'
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
        <ShellSidebar
          :active-page="currentPage"
          :active-remote-work-count="activeRemoteWorkCount"
          :remote-agent-configured="Boolean(remoteAgentSettings?.configured)"
          :review-count="reviewCount"
          :runner-setup-ready="runnerSetupReady"
          :total-project-count="totalProjectCount"
          :visible-task-count="visibleTaskCount"
          @navigate="currentPage = $event"
        />

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

    <ShellOverlayMount
      :available-projects="availableProjects"
      :canceling-dispatch-task-id="cancelingDispatchTaskId"
      :canceling-review-id="cancelingReviewId"
      :cleanup-pending-confirmation="cleanupPendingConfirmation"
      :cleaning-up-remote-artifacts="cleaningUpRemoteArtifacts"
      :creating-review="creatingReview"
      :creating-task="creatingTask"
      :default-create-project="defaultCreateProject"
      :default-remote-agent-preferred-tool="defaultRemoteAgentPreferredTool"
      :dispatching-task-id="dispatchingTaskId"
      :discarding-dispatch-task-id="discardingDispatchTaskId"
      :editing-project="editingProject"
      :editing-remote-agent-setup="editingRemoteAgentSetup"
      :editing-task="editingTask"
      :following-up-dispatch="followingUpDispatch"
      :following-up-review="followingUpReview"
      :following-up-review-id="followingUpReviewId"
      :following-up-task="followingUpTask"
      :following-up-task-id="followingUpTaskId"
      :remote-agent-settings="remoteAgentSettings"
      :reset-pending-confirmation="resetPendingConfirmation"
      :resetting-remote-workspace="resettingRemoteWorkspace"
      :review-pending-deletion="reviewPendingDeletion"
      :runner-setup-required-for-dispatch="taskPendingRunnerSetup !== null"
      :saving="saving"
      :selected-review="selectedReview"
      :selected-review-can-cancel="selectedReviewCanCancel"
      :selected-review-can-re-review="selectedReviewCanReReview"
      :selected-review-latest-run="selectedReviewLatestRun"
      :selected-review-runs="selectedReviewRuns"
      :selected-task="selectedTask"
      :selected-task-can-continue="selectedTaskCanContinue"
      :selected-task-can-discard-history="selectedTaskCanDiscardHistory"
      :selected-task-can-start-fresh="selectedTaskCanStartFresh"
      :selected-task-dispatch-disabled-reason="selectedTaskDispatchDisabledReason"
      :selected-task-dispatch-tool="selectedTaskDispatchTool"
      :selected-task-latest-dispatch="selectedTaskLatestDispatch"
      :selected-task-latest-reusable-pull-request="selectedTaskLatestReusablePullRequest"
      :selected-task-lifecycle-message="selectedTaskLifecycleMessage"
      :selected-task-lifecycle-mutation="selectedTaskLifecycleMutation"
      :selected-task-pinned-tool="selectedTaskPinnedTool"
      :selected-task-primary-action-disabled="selectedTaskPrimaryActionDisabled"
      :selected-task-project="selectedTaskProject"
      :selected-task-runs="selectedTaskRuns"
      :show-review-drawer="currentPage === 'reviews' && isReviewDrawerOpen && selectedReview !== null"
      :show-task-drawer="currentPage === 'tasks' && isTaskDrawerOpen && selectedTask !== null"
      :task-pending-deletion="taskPendingDeletion"
      @cancel-cleanup="clearPendingRemoteCleanup"
      @cancel-project-editor="closeProjectEditor"
      @cancel-reset="clearPendingRemoteReset"
      @cancel-review-delete="clearPendingReviewDeletion"
      @cancel-review-drawer="closeReviewDrawer"
      @cancel-review-editor="closeReviewEditor"
      @cancel-review-follow-up="closeReviewFollowUpEditor"
      @cancel-runner-setup="closeRunnerSetup"
      @cancel-task-delete="clearPendingDeletion"
      @cancel-task-drawer="closeTaskDrawer"
      @cancel-task-editor="closeTaskEditor"
      @cancel-task-follow-up="closeFollowUpEditor"
      @confirm-cleanup="confirmRemoteCleanup"
      @confirm-reset="confirmRemoteReset"
      @confirm-review-delete="confirmReviewDelete"
      @confirm-task-delete="confirmDelete"
      @request-cancel-review-run="cancelReviewRun"
      @request-delete-review="queueReviewDeletion"
      @request-edit-task="openTaskEditor"
      @request-open-task-project="openSelectedTaskProjectDetails"
      @request-open-url="openExternal"
      @request-review-follow-up="openReviewFollowUpEditor"
      @request-save-project="saveProjectEdits"
      @request-save-review="createReviewFromWeb"
      @request-save-review-follow-up="submitReviewFollowUp"
      @request-save-runner-setup="saveRemoteAgentSetup"
      @request-save-task="creatingTask ? createTaskFromWeb($event) : saveTaskEdits($event)"
      @request-save-task-follow-up="submitFollowUp"
      @request-selected-task-close="updateTaskStatus($event, 'closed')"
      @request-selected-task-delete="queueTaskDeletion"
      @request-selected-task-discard-history="discardRunHistory"
      @request-selected-task-primary-action="handlePrimaryAction"
      @request-selected-task-start-fresh="startRemoteRun"
      @update:task-start-tool="selectedTaskStartTool = $event"
    />
  </main>
</template>
