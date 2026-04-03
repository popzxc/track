<script setup lang="ts">
import { computed, ref } from 'vue'

import { ApiClientError } from './api/client'
import MigrationStatePanel from './components/MigrationStatePanel.vue'
import ProjectsScreen from './components/ProjectsScreen.vue'
import ReviewsScreen from './components/ReviewsScreen.vue'
import RunsScreen from './components/RunsScreen.vue'
import ShellSidebar from './components/ShellSidebar.vue'
import SettingsScreen from './components/SettingsScreen.vue'
import TasksScreen from './components/TasksScreen.vue'
import { useAppDataLoader } from './composables/useAppDataLoader'
import { useBackgroundSync } from './composables/useBackgroundSync'
import { useWorkflowScreens } from './composables/useWorkflowScreens'
import { mergeProjects } from './features/tasks/presentation'
import type {
  MigrationImportSummary,
  MigrationStatus,
  ProjectInfo,
  RemoteAgentPreferredTool,
  RemoteAgentSettings,
  ReviewSummary,
  RunRecord,
  Task,
} from './types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

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
const remoteAgentSettings = ref<RemoteAgentSettings | null>(null)
const loading = ref(true)
const refreshing = ref(false)
const saving = ref(false)
const errorMessage = ref('')
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
const shellPreludeHelpText = 'The remote runner uses non-interactive SSH sessions, so it cannot rely on the environment tweaks that usually live in your interactive shell.\n\nKeep the shell prelude focused on PATH and toolchain setup. The backend reuses it before every remote command so dispatches stay predictable.'

// Tasks, reviews, and runs still share one legitimate integration cycle: user
// selection drives history loading, and optimistic run writes feed back into
// those same surfaces. Keeping that wiring behind one named composable makes
// the shell read as composition again instead of a sequence of bridge vars.
const workflowScreens = useWorkflowScreens({
  availableProjects,
  canRequestReview,
  currentPage,
  defaultRemoteAgentPreferredTool,
  errorMessage,
  migrationImportPending,
  migrationImportSummary,
  migrationStatus,
  remoteAgentSettings,
  reviewRequestDisabledReason,
  runnerSetupReady,
  saving,
  setFriendlyError,
  shellPreludeHelpText,
  tasks,
  reviews,
  runs,
})

function setFriendlyError(error: unknown) {
  if (error instanceof ApiClientError) {
    errorMessage.value = error.message
    return
  }

  errorMessage.value =
    error instanceof Error ? error.message : 'Something went wrong while talking to the API.'
}

let syncTaskChangeVersion = async () => undefined

const {
  loadRemoteAgentSettings,
  loadTasks,
  refreshAll,
} = useAppDataLoader({
  errorMessage,
  latestTaskDispatchesByTaskId: workflowScreens.tasksScreen.latestTaskDispatchesByTaskId,
  loading,
  loadLatestDispatchesForVisibleTasks: workflowScreens.loadLatestDispatchesForVisibleTasks,
  loadReviews: workflowScreens.loadReviews,
  loadRuns: workflowScreens.loadRuns,
  loadSelectedReviewRunHistory: workflowScreens.loadSelectedReviewRunHistory,
  loadSelectedTaskRunHistory: workflowScreens.loadSelectedTaskRunHistory,
  migrationStatus,
  projects,
  refreshing,
  remoteAgentSettings,
  reviews,
  runs,
  selectedProjectFilter: workflowScreens.tasksScreen.selectedProjectFilter,
  selectedReviewRuns: workflowScreens.reviewsScreen.selectedReviewRuns,
  selectedTaskRuns: workflowScreens.tasksScreen.selectedTaskRuns,
  setFriendlyError,
  showClosed: workflowScreens.tasksScreen.showClosed,
  syncTaskChangeVersion: () => syncTaskChangeVersion(),
  taskProjectOptions,
  tasks,
})
workflowScreens.connectDataLoader({
  loadRemoteAgentSettings,
  refreshAll,
})

const backgroundSync = useBackgroundSync({
  activeReviewRuns: workflowScreens.activeReviewRuns,
  activeRuns: workflowScreens.activeRuns,
  cancelingDispatchTaskId: workflowScreens.tasksScreen.cancelingDispatchTaskId,
  cancelingReviewId: workflowScreens.reviewsScreen.cancelingReviewId,
  dispatchingTaskId: workflowScreens.tasksScreen.dispatchingTaskId,
  discardingDispatchTaskId: workflowScreens.tasksScreen.discardingDispatchTaskId,
  followingUpTaskId: workflowScreens.tasksScreen.followingUpTaskId,
  isReviewDrawerOpen: workflowScreens.reviewsScreen.isReviewDrawerOpen,
  isTaskDrawerOpen: workflowScreens.tasksScreen.isTaskDrawerOpen,
  loading,
  loadLatestDispatchesForVisibleTasks: workflowScreens.loadLatestDispatchesForVisibleTasks,
  loadReviews: workflowScreens.loadReviews,
  loadRuns: workflowScreens.loadRuns,
  loadSelectedReviewRunHistory: workflowScreens.loadSelectedReviewRunHistory,
  loadSelectedTaskRunHistory: workflowScreens.loadSelectedTaskRunHistory,
  loadTasks,
  refreshAll,
  refreshing,
  saving,
  selectedProjectFilter: workflowScreens.tasksScreen.selectedProjectFilter,
  selectedReview: workflowScreens.reviewsScreen.selectedReview,
  selectedReviewRuns: workflowScreens.reviewsScreen.selectedReviewRuns,
  selectedTask: workflowScreens.tasksScreen.selectedTask,
  selectedTaskRuns: workflowScreens.tasksScreen.selectedTaskRuns,
  setFriendlyError,
  showClosed: workflowScreens.tasksScreen.showClosed,
})
syncTaskChangeVersion = backgroundSync.syncTaskChangeVersion
</script>

<template>
  <main class="min-h-screen px-4 py-4 sm:px-6 sm:py-6 lg:px-8">
    <div class="mx-auto max-w-[1800px]">
      <div class="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
        <ShellSidebar
          :active-page="currentPage"
          :active-remote-work-count="workflowScreens.activeRemoteWorkCount.value"
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
              @request-import-legacy-data="workflowScreens.importLegacyTrackerData"
            />

            <TasksScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'tasks'"
              :controller="workflowScreens.tasksScreen"
            />

            <ReviewsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'reviews'"
              :controller="workflowScreens.reviewsScreen"
            />

            <RunsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'runs'"
              :controller="workflowScreens.runsScreen"
            />

            <ProjectsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'projects'"
              :controller="workflowScreens.projectsScreen"
            />

            <SettingsScreen
              v-if="!migrationGateActive"
              :active="currentPage === 'settings'"
              :controller="workflowScreens.settingsScreen"
            />
          </template>
        </section>
      </div>
    </div>
  </main>
</template>
