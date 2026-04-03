<script setup lang="ts">
import { computed, ref } from 'vue'

import { ApiClientError, dispatchTask } from './api/client'
import MigrationStatePanel from './components/MigrationStatePanel.vue'
import ProjectsScreen from './components/ProjectsScreen.vue'
import ReviewsScreen from './components/ReviewsScreen.vue'
import RunsScreen from './components/RunsScreen.vue'
import ShellSidebar from './components/ShellSidebar.vue'
import SettingsScreen from './components/SettingsScreen.vue'
import TasksScreen from './components/TasksScreen.vue'
import { useAppDataLoader } from './composables/useAppDataLoader'
import { useBackgroundSync } from './composables/useBackgroundSync'
import { useProjectViewState } from './composables/useProjectViewState'
import { useReviewViewState } from './composables/useReviewViewState'
import { useRunState } from './composables/useRunState'
import { useSettingsMutations } from './composables/useSettingsMutations'
import type { PendingRunnerSetupRequest } from './composables/useTaskMutations'
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

async function resumeQueuedTaskDispatch(task: Task, preferredTool: RemoteAgentPreferredTool) {
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
    // This callback still lives in the shell because runner setup is a
    // cross-screen flow: task dispatch queues work from Tasks, then Settings
    // resumes it after the shell prelude has been saved.
    void resumeQueuedTaskDispatch(task, preferredTool)
  },
  saving,
  setFriendlyError,
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

// These context bags intentionally group refs by domain so App.vue can hand a
// whole screen its slice of shell state instead of flattening dozens of props
// through an overlay hub. The screens still consume refs directly, which keeps
// this step incremental while making ownership boundaries explicit.
const tasksScreenContext = {
  availableProjects,
  cancelingDispatchTaskId,
  closeTaskDrawer,
  creatingTask,
  currentPage,
  defaultCreateProject,
  dispatchingTaskId,
  discardingDispatchTaskId,
  editingRemoteAgentSetup,
  editingTask,
  errorMessage,
  followingUpDispatch,
  followingUpTask,
  followingUpTaskId,
  isTaskDrawerOpen,
  latestTaskDispatchesByTaskId,
  loadRemoteAgentSettings,
  loadRuns,
  openSelectedTaskProjectDetails,
  pendingSelectedTaskId,
  refreshAll,
  remoteAgentSettings,
  removeTaskRuns,
  runnerSetupReady,
  saving,
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
  selectedTaskRuns,
  selectedTaskStartTool,
  selectTask,
  setFriendlyError,
  showClosed,
  taskGroups,
  taskLifecycleMutation,
  taskLifecycleMutationTaskId,
  taskPendingDeletion,
  taskPendingRunnerSetup,
  tasks,
  upsertLatestTaskDispatch,
  upsertRunRecord,
  upsertSelectedTaskRun,
}

const reviewsScreenContext = {
  cancelingReviewId,
  canRequestReview,
  closeReviewDrawer,
  creatingReview,
  currentPage,
  defaultRemoteAgentPreferredTool,
  errorMessage,
  followingUpReview,
  followingUpReviewId,
  isReviewDrawerOpen,
  refreshAll,
  remoteAgentSettings,
  removeReview,
  replaceSelectedReviewRuns,
  reviewPendingDeletion,
  reviewRequestDisabledReason,
  reviews,
  saving,
  selectedReview,
  selectedReviewCanCancel,
  selectedReviewCanReReview,
  selectedReviewLatestRun,
  selectedReviewRuns,
  selectReview,
  setFriendlyError,
  upsertLatestReviewRun,
  upsertReviewSummary,
  upsertSelectedReviewRun,
}

const runsScreenContext = {
  activeReviewRuns,
  activeRuns,
  openTaskFromRun,
  recentReviewRuns,
  recentRuns,
  selectReview,
}

const projectsScreenContext = {
  availableProjects,
  editingProject,
  saveProjectEdits,
  saving,
  selectedProjectDetails,
  selectedProjectDetailsId,
}

const settingsScreenContext = {
  activeRemoteWorkCount,
  cleaningUpRemoteArtifacts,
  cleanupPendingConfirmation,
  cleanupSummary,
  confirmRemoteCleanup,
  confirmRemoteReset,
  editingRemoteAgentSetup,
  remoteAgentSettings,
  resetPendingConfirmation,
  resettingRemoteWorkspace,
  resetSummary,
  runnerSetupReady,
  saveRemoteAgentSetup,
  saving,
  shellPreludeHelpText,
  taskPendingRunnerSetup,
}
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

            <TasksScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'tasks'"
              :context="tasksScreenContext"
            />

            <ReviewsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'reviews'"
              :context="reviewsScreenContext"
            />

            <RunsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'runs'"
              :context="runsScreenContext"
            />

            <ProjectsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'projects'"
              :context="projectsScreenContext"
            />

            <SettingsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'settings'"
              :context="settingsScreenContext"
            />
          </template>
        </section>
      </div>
    </div>
  </main>
</template>
