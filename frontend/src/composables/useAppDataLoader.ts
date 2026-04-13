import type { Ref } from 'vue'

import { fetchProjects, fetchRemoteAgentSettings, fetchTasks } from '../api/client'
import type {
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
 * Owns the shell's refresh policy.
 *
 * The frontend does not have a single "load everything" endpoint because the
 * backend's persisted truth spans tasks, reviews, dispatch history, and runner
 * settings. This composable centralizes how those slices are refreshed
 * together.
 */
export function useAppDataLoader(options: UseAppDataLoaderOptions) {
  async function loadProjects() {
    options.projects.value = await fetchProjects()
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
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.loading.value = false
      options.refreshing.value = false
    }
  }

  return {
    loadProjects,
    loadRemoteAgentSettings,
    loadTasks,
    refreshAll,
  }
}
