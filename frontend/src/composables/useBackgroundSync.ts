import { onBeforeUnmount, onMounted, ref, watch, type ComputedRef, type Ref } from 'vue'

import { fetchTaskChangeVersion } from '../api/client'
import type {
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
} from '../types/task'

interface UseBackgroundSyncOptions {
  activeReviewRuns: ComputedRef<ReviewSummary[]>
  activeRuns: ComputedRef<RunRecord[]>
  cancelingDispatchTaskId: Ref<string | null>
  cancelingReviewId: Ref<string | null>
  dispatchingTaskId: Ref<string | null>
  discardingDispatchTaskId: Ref<string | null>
  followingUpTaskId: Ref<string | null>
  isReviewDrawerOpen: Ref<boolean>
  isTaskDrawerOpen: Ref<boolean>
  loading: Ref<boolean>
  loadLatestDispatchesForVisibleTasks: () => Promise<void>
  loadReviews: () => Promise<void>
  loadRuns: () => Promise<void>
  loadSelectedReviewRunHistory: () => Promise<void>
  loadSelectedTaskRunHistory: () => Promise<void>
  loadTasks: () => Promise<void>
  refreshAll: () => Promise<void>
  refreshing: Ref<boolean>
  saving: Ref<boolean>
  selectedProjectFilter: Ref<string>
  selectedReview: ComputedRef<ReviewRecord | null>
  selectedReviewRuns: Ref<ReviewRunRecord[]>
  selectedTask: ComputedRef<Task | null>
  selectedTaskRuns: Ref<RunRecord[]>
  setFriendlyError: (error: unknown) => void
  showClosed: Ref<boolean>
}

const TASK_CHANGE_POLL_INTERVAL_MS = 2_000
const RUN_POLL_INTERVAL_MS = 60_000

/**
 * Coordinates background refresh for the shell's filesystem-backed data.
 *
 * `track` has two refresh cadences with different user expectations:
 * task files should appear quickly because local capture is a primary workflow,
 * while remote run status can refresh more slowly because it is inherently
 * background work. This composable owns that policy so App.vue can describe
 * mutations and view state without also managing timers and poll guards.
 */
export function useBackgroundSync(options: UseBackgroundSyncOptions) {
  const taskChangeVersion = ref<number | null>(null)

  let taskChangePollTimer: number | null = null
  let taskChangePollInFlight = false
  let runPollTimer: number | null = null
  let runPollInFlight = false

  async function syncTaskChangeVersion() {
    taskChangeVersion.value = await fetchTaskChangeVersion()
  }

  async function pollForTaskChanges() {
    if (
      taskChangePollInFlight ||
      options.loading.value ||
      options.refreshing.value ||
      options.saving.value ||
      options.dispatchingTaskId.value !== null ||
      options.cancelingDispatchTaskId.value !== null ||
      options.cancelingReviewId.value !== null ||
      options.discardingDispatchTaskId.value !== null ||
      options.followingUpTaskId.value !== null
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
        await options.refreshAll()
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
      options.loading.value ||
      options.refreshing.value ||
      options.saving.value ||
      options.dispatchingTaskId.value !== null ||
      options.cancelingDispatchTaskId.value !== null ||
      options.cancelingReviewId.value !== null ||
      options.discardingDispatchTaskId.value !== null ||
      options.followingUpTaskId.value !== null
    ) {
      return
    }

    if (options.activeRuns.value.length === 0 && options.activeReviewRuns.value.length === 0) {
      return
    }

    runPollInFlight = true

    try {
      await Promise.all([
        options.loadRuns(),
        options.loadReviews(),
        options.loadLatestDispatchesForVisibleTasks(),
        options.loadSelectedTaskRunHistory().catch(() => {
          // The rest of the run state remains useful even if the drawer
          // history fails to refresh on one background poll.
        }),
        options.loadSelectedReviewRunHistory().catch(() => {
          // Review history is secondary to the latest status cards, so this
          // poll stays best-effort for the review drawer as well.
        }),
      ])
    } catch {
      // The last known run state remains useful, so this poll stays best-effort.
    } finally {
      runPollInFlight = false
    }
  }

  watch([options.showClosed, options.selectedProjectFilter], () => {
    if (options.loading.value) {
      return
    }

    void (async () => {
      try {
        await options.loadTasks()
        await Promise.all([
          options.loadLatestDispatchesForVisibleTasks(),
          options.loadSelectedTaskRunHistory().catch(() => {
            // Changing filters should not blank the queue just because drawer
            // history could not be refreshed for the currently selected task.
          }),
        ])
      } catch (error) {
        options.setFriendlyError(error)
      }
    })()
  })

  watch([options.isTaskDrawerOpen, options.selectedTask], ([drawerOpen, task]) => {
    if (!task) {
      options.selectedTaskRuns.value = []
      return
    }

    if (!drawerOpen) {
      options.selectedTaskRuns.value = []
      return
    }

    void options.loadSelectedTaskRunHistory().catch(options.setFriendlyError)
  })

  watch([options.isReviewDrawerOpen, options.selectedReview], ([drawerOpen, review]) => {
    if (!review) {
      options.selectedReviewRuns.value = []
      return
    }

    if (!drawerOpen) {
      options.selectedReviewRuns.value = []
      return
    }

    void options.loadSelectedReviewRunHistory().catch(options.setFriendlyError)
  })

  onMounted(() => {
    void options.refreshAll()

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

  return {
    pollForRunChanges,
    pollForTaskChanges,
    syncTaskChangeVersion,
    taskChangeVersion,
  }
}
