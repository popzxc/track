import type { ComputedRef } from 'vue'

import type {
  ReviewSummary,
  RunRecord,
} from '../types/task'

interface UseRunsScreenControllerOptions {
  activeReviewRuns: ComputedRef<ReviewSummary[]>
  activeRuns: ComputedRef<RunRecord[]>
  openTaskFromRun: (run: RunRecord) => void
  recentReviewRuns: ComputedRef<ReviewSummary[]>
  recentRuns: ComputedRef<RunRecord[]>
  selectReview: (reviewId: string) => void
}

/**
 * Keeps the runs screen on the same controller pattern as the other screens.
 *
 * The runs surface is already fairly small, so this controller is mostly a
 * naming boundary that keeps App.vue uniformly declarative.
 */
export function useRunsScreenController(options: UseRunsScreenControllerOptions) {
  return {
    activeReviewRuns: options.activeReviewRuns,
    activeRuns: options.activeRuns,
    openTaskFromRun: options.openTaskFromRun,
    recentReviewRuns: options.recentReviewRuns,
    recentRuns: options.recentRuns,
    selectReview: options.selectReview,
  }
}

export type RunsScreenController = ReturnType<typeof useRunsScreenController>
