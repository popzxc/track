import type { Ref } from 'vue'

import {
  ApiClientError,
  fetchMigrationStatus,
  fetchProjects,
  fetchRemoteAgentSettings,
  fetchTasks,
} from '../api/client'
import type {
  MigrationStatus,
  ProjectInfo,
  RemoteAgentSettings,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'

interface UseAppDataLoaderOptions {
  errorMessage: Ref<string>
  latestTaskDispatchesByTaskId: Ref<Record<string, TaskDispatch>>
  loading: Ref<boolean>
  loadLatestDispatchesForVisibleTasks: () => Promise<void>
  loadReviews: () => Promise<void>
  loadRuns: () => Promise<void>
  loadSelectedReviewRunHistory: () => Promise<void>
  loadSelectedTaskRunHistory: () => Promise<void>
  migrationStatus: Ref<MigrationStatus | null>
  projects: Ref<ProjectInfo[]>
  refreshing: Ref<boolean>
  remoteAgentSettings: Ref<RemoteAgentSettings | null>
  reviews: Ref<ReviewSummary[]>
  runs: Ref<RunRecord[]>
  selectedProjectFilter: Ref<string>
  selectedReviewRuns: Ref<ReviewRunRecord[]>
  selectedTaskRuns: Ref<RunRecord[]>
  setFriendlyError: (error: unknown) => void
  showClosed: Ref<boolean>
  syncTaskChangeVersion: () => Promise<void>
  taskProjectOptions: Ref<ProjectInfo[]>
  tasks: Ref<Task[]>
}

/**
 * Owns the shell's refresh policy and migration-gated bootstrap.
 *
 * The frontend does not have a single "load everything" endpoint because the
 * backend's persisted truth spans tasks, reviews, dispatch history, and runner
 * settings. This composable centralizes how those slices are refreshed
 * together, including the special case where normal API routes stay gated until
 * legacy data is imported into SQLite.
 */
export function useAppDataLoader(options: UseAppDataLoaderOptions) {
  async function loadProjects() {
    options.projects.value = await fetchProjects()
  }

  async function loadMigrationGate() {
    options.migrationStatus.value = await fetchMigrationStatus()
  }

  async function loadRemoteAgentSettings() {
    options.remoteAgentSettings.value = await fetchRemoteAgentSettings()
  }

  async function loadTasks() {
    options.tasks.value = await fetchTasks({
      includeClosed: options.showClosed.value,
      project: options.selectedProjectFilter.value || undefined,
    })

    // Tasks may reference projects the explicit project registry has not loaded
    // yet. We synthesize lightweight entries here so the queue filters remain
    // complete even before project metadata catches up.
    options.taskProjectOptions.value = options.tasks.value.map((task) => ({
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

  function resetAppDataForMigration() {
    options.tasks.value = []
    options.reviews.value = []
    options.projects.value = []
    options.taskProjectOptions.value = []
    options.runs.value = []
    options.latestTaskDispatchesByTaskId.value = {}
    options.selectedTaskRuns.value = []
    options.selectedReviewRuns.value = []
    options.remoteAgentSettings.value = null
  }

  async function refreshAll() {
    options.errorMessage.value = ''
    options.refreshing.value = true

    try {
      await Promise.all([
        loadProjects(),
        loadTasks(),
        options.loadReviews(),
        options.syncTaskChangeVersion(),
        loadRemoteAgentSettings().catch(() => {
          // Runner setup is useful context, but the rest of the app should still
          // render if that endpoint is temporarily unavailable.
        }),
      ])

      // The local-first shell data above is enough to render the queue. We
      // intentionally keep the slower dispatch/run enrichment on the same
      // refresh cycle, but the initial screen no longer waits for it.
      options.loading.value = false

      await Promise.all([
        options.loadLatestDispatchesForVisibleTasks(),
        options.loadRuns(),
        options.loadSelectedTaskRunHistory().catch(() => {
          // The drawer can still show the task body if task-scoped run history
          // is temporarily unavailable.
        }),
        options.loadSelectedReviewRunHistory().catch(() => {
          // The review drawer can still show the persisted review record if its
          // run history is temporarily unavailable.
        }),
      ])
      options.migrationStatus.value = null
    } catch (error) {
      if (error instanceof ApiClientError && error.code === 'MIGRATION_REQUIRED') {
        try {
          await loadMigrationGate()
          resetAppDataForMigration()
        } catch (migrationError) {
          options.setFriendlyError(migrationError)
        }
      } else {
        options.setFriendlyError(error)
      }
    } finally {
      options.loading.value = false
      options.refreshing.value = false
    }
  }

  return {
    loadMigrationGate,
    loadProjects,
    loadRemoteAgentSettings,
    loadTasks,
    refreshAll,
    resetAppDataForMigration,
  }
}
