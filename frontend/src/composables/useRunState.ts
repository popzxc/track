import { computed, ref, type Ref } from 'vue'

import {
  fetchDispatches,
  fetchReviewRuns,
  fetchReviews,
  fetchRuns,
  fetchTaskRuns,
} from '../api/client'
import type {
  DispatchStatus,
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'

interface UseRunStateOptions {
  tasks: Ref<Task[]>
}

const ACTIVE_DISPATCH_STATUSES = new Set<DispatchStatus>(['preparing', 'running'])

function isActiveDispatchStatus(status: DispatchStatus) {
  return ACTIVE_DISPATCH_STATUSES.has(status)
}

function reviewSummaryTimestamp(summary: ReviewSummary) {
  return summary.latestRun?.createdAt ?? summary.review.updatedAt ?? summary.review.createdAt
}

function taskRunTimestamp(run: RunRecord) {
  return run.dispatch.createdAt
}

function reviewRunTimestamp(run: ReviewRunRecord) {
  return run.createdAt
}

function sortByNewestTimestamp<T>(items: T[], timestamp: (item: T) => string) {
  return items
    .slice()
    .sort((left, right) => Date.parse(timestamp(right)) - Date.parse(timestamp(left)))
}

function sortReviewSummaries(reviewSummaries: ReviewSummary[]) {
  return sortByNewestTimestamp(reviewSummaries, reviewSummaryTimestamp)
}

export function sortTaskRunRecords(runs: RunRecord[]) {
  return sortByNewestTimestamp(runs, taskRunTimestamp)
}

export function sortReviewRunRecords(runs: ReviewRunRecord[]) {
  return sortByNewestTimestamp(runs, reviewRunTimestamp)
}

export function upsertTaskRunRecord(
  runs: RunRecord[],
  task: Task,
  dispatch: TaskDispatch,
) {
  return sortTaskRunRecords([
    { task, dispatch },
    ...runs.filter((run) => run.dispatch.dispatchId !== dispatch.dispatchId),
  ])
}

export function upsertReviewRunRecord(
  runs: ReviewRunRecord[],
  run: ReviewRunRecord,
) {
  return sortReviewRunRecords([
    run,
    ...runs.filter((entry) => entry.dispatchId !== run.dispatchId),
  ])
}

/**
 * Owns the frontend's global run and review projections.
 *
 * The backend remains the source of truth, but the shell still needs one local
 * model that can answer "what is active?", "what is recent?", and "what is the
 * latest dispatch for each visible task?" Keeping those rules here prevents
 * route pages, mutations, and background polls from each inventing their own
 * ordering and merge behavior.
 */
export function useRunState(options: UseRunStateOptions) {
  const reviews = ref<ReviewSummary[]>([])
  const runs = ref<RunRecord[]>([])
  const latestTaskDispatchesByTaskId = ref<Record<string, TaskDispatch>>({})

  const activeRuns = computed(() =>
    sortTaskRunRecords(
      runs.value.filter((run) => isActiveDispatchStatus(run.dispatch.status)),
    ),
  )

  const activeReviewRuns = computed(() =>
    sortReviewSummaries(
      reviews.value.filter((summary) =>
        summary.latestRun ? isActiveDispatchStatus(summary.latestRun.status) : false,
      ),
    ),
  )

  const recentRuns = computed(() => sortTaskRunRecords(runs.value).slice(0, 40))

  const recentReviewRuns = computed(() =>
    sortReviewSummaries(reviews.value.filter((summary) => Boolean(summary.latestRun))).slice(0, 40),
  )

  async function loadReviews() {
    reviews.value = sortReviewSummaries(await fetchReviews())
  }

  async function loadRuns() {
    runs.value = sortTaskRunRecords(await fetchRuns(200))
  }

  async function loadLatestDispatchesForVisibleTasks() {
    const dispatches = await fetchDispatches(options.tasks.value.map((task) => task.id))

    const latestByTaskId: Record<string, TaskDispatch> = {}
    for (const dispatch of dispatches) {
      latestByTaskId[dispatch.taskId] = dispatch
    }

    latestTaskDispatchesByTaskId.value = latestByTaskId
  }

  async function loadTaskRuns(taskId: string) {
    return sortTaskRunRecords(await fetchTaskRuns(taskId))
  }

  async function loadReviewRuns(reviewId: string) {
    return sortReviewRunRecords(await fetchReviewRuns(reviewId))
  }

  function upsertRunRecord(task: Task, dispatch: TaskDispatch) {
    runs.value = upsertTaskRunRecord(runs.value, task, dispatch)
  }

  function upsertLatestTaskDispatch(dispatch: TaskDispatch) {
    latestTaskDispatchesByTaskId.value = {
      ...latestTaskDispatchesByTaskId.value,
      [dispatch.taskId]: dispatch,
    }
  }

  function removeTaskRuns(taskId: string) {
    runs.value = runs.value.filter((run) => run.task.id !== taskId)
    const nextDispatches = { ...latestTaskDispatchesByTaskId.value }
    delete nextDispatches[taskId]
    latestTaskDispatchesByTaskId.value = nextDispatches
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

  function removeReview(reviewId: string) {
    reviews.value = reviews.value.filter((summary) => summary.review.id !== reviewId)
  }

  return {
    activeReviewRuns,
    activeRuns,
    latestTaskDispatchesByTaskId,
    loadLatestDispatchesForVisibleTasks,
    loadReviewRuns,
    loadReviews,
    loadRuns,
    loadTaskRuns,
    recentReviewRuns,
    recentRuns,
    removeReview,
    removeTaskRuns,
    reviews,
    runs,
    upsertLatestReviewRun,
    upsertLatestTaskDispatch,
    upsertReviewSummary,
    upsertRunRecord,
  }
}
