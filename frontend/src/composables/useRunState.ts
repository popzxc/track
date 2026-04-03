import { computed, type ComputedRef, type Ref } from 'vue'

import {
  fetchDispatches,
  fetchReviewRuns,
  fetchReviews,
  fetchRuns,
  fetchTaskRuns,
} from '../api/client'
import type {
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'

interface UseRunStateOptions {
  closeReviewDrawer: () => void
  isReviewDrawerOpen: Ref<boolean>
  isTaskDrawerOpen: Ref<boolean>
  latestTaskDispatchesByTaskId: Ref<Record<string, TaskDispatch>>
  reviews: Ref<ReviewSummary[]>
  runs: Ref<RunRecord[]>
  selectedReview: ComputedRef<ReviewRecord | null>
  selectedReviewId: Ref<string | null>
  selectedReviewRuns: Ref<ReviewRunRecord[]>
  selectedTask: ComputedRef<Task | null>
  selectedTaskId: Ref<string | null>
  selectedTaskRuns: Ref<RunRecord[]>
  tasks: Ref<Task[]>
}

function reviewSummaryTimestamp(summary: ReviewSummary) {
  return summary.latestRun?.createdAt ?? summary.review.updatedAt ?? summary.review.createdAt
}

function sortReviewSummaries(reviewSummaries: ReviewSummary[]) {
  return reviewSummaries
    .slice()
    .sort((left, right) => Date.parse(reviewSummaryTimestamp(right)) - Date.parse(reviewSummaryTimestamp(left)))
}

/**
 * Owns the frontend's in-memory run and review projections.
 *
 * The backend remains the source of truth, but the shell still needs a local
 * model that can answer "what is the latest run for this task?", merge fresh
 * optimistic responses into the visible history, and load drawer-scoped history
 * without clobbering a newer selection. Keeping those rules together avoids
 * duplicating temporal bookkeeping across mutations and polls.
 */
export function useRunState(options: UseRunStateOptions) {
  let selectedTaskRunsRequestVersion = 0
  let selectedReviewRunsRequestVersion = 0

  const activeRuns = computed(() =>
    options.runs.value
      .filter((run) => run.dispatch.status === 'preparing' || run.dispatch.status === 'running')
      .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt)),
  )

  const activeReviewRuns = computed(() =>
    options.reviews.value
      .filter(
        (summary) =>
          summary.latestRun?.status === 'preparing' || summary.latestRun?.status === 'running',
      )
      .sort((left, right) => {
        const leftCreatedAt = left.latestRun?.createdAt ?? left.review.updatedAt ?? left.review.createdAt
        const rightCreatedAt = right.latestRun?.createdAt ?? right.review.updatedAt ?? right.review.createdAt
        return Date.parse(rightCreatedAt) - Date.parse(leftCreatedAt)
      }),
  )

  const recentRuns = computed(() =>
    options.runs.value
      .slice()
      .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
      .slice(0, 40),
  )

  const recentReviewRuns = computed(() =>
    options.reviews.value
      .filter((summary) => Boolean(summary.latestRun))
      .slice()
      .sort((left, right) => {
        const leftCreatedAt = left.latestRun?.createdAt ?? left.review.updatedAt ?? left.review.createdAt
        const rightCreatedAt = right.latestRun?.createdAt ?? right.review.updatedAt ?? right.review.createdAt
        return Date.parse(rightCreatedAt) - Date.parse(leftCreatedAt)
      })
      .slice(0, 40),
  )

  function replaceSelectedTaskRuns(taskRuns: RunRecord[]) {
    options.selectedTaskRuns.value = taskRuns
      .slice()
      .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
  }

  function replaceSelectedReviewRuns(reviewRuns: ReviewRunRecord[]) {
    options.selectedReviewRuns.value = reviewRuns
      .slice()
      .sort((left, right) => Date.parse(right.createdAt) - Date.parse(left.createdAt))
  }

  async function loadReviews() {
    options.reviews.value = sortReviewSummaries(await fetchReviews())
  }

  async function loadRuns() {
    options.runs.value = await fetchRuns(200)
  }

  async function loadLatestDispatchesForVisibleTasks() {
    const dispatches = await fetchDispatches(options.tasks.value.map((task) => task.id))

    const latestByTaskId: Record<string, TaskDispatch> = {}
    for (const dispatch of dispatches) {
      latestByTaskId[dispatch.taskId] = dispatch
    }

    options.latestTaskDispatchesByTaskId.value = latestByTaskId
  }

  async function loadSelectedTaskRunHistory() {
    if (!options.isTaskDrawerOpen.value || !options.selectedTask.value) {
      options.selectedTaskRuns.value = []
      return
    }

    const requestVersion = ++selectedTaskRunsRequestVersion
    const taskId = options.selectedTask.value.id
    const taskRuns = await fetchTaskRuns(taskId)

    if (
      requestVersion !== selectedTaskRunsRequestVersion ||
      !options.isTaskDrawerOpen.value ||
      options.selectedTask.value?.id !== taskId
    ) {
      return
    }

    replaceSelectedTaskRuns(taskRuns)
  }

  async function loadSelectedReviewRunHistory() {
    if (!options.isReviewDrawerOpen.value || !options.selectedReview.value) {
      options.selectedReviewRuns.value = []
      return
    }

    const requestVersion = ++selectedReviewRunsRequestVersion
    const reviewId = options.selectedReview.value.id
    const reviewRuns = await fetchReviewRuns(reviewId)

    if (
      requestVersion !== selectedReviewRunsRequestVersion ||
      !options.isReviewDrawerOpen.value ||
      options.selectedReview.value?.id !== reviewId
    ) {
      return
    }

    replaceSelectedReviewRuns(reviewRuns)
  }

  function upsertRunRecord(task: Task, dispatch: TaskDispatch) {
    const nextRecord: RunRecord = { task, dispatch }
    options.runs.value = [nextRecord, ...options.runs.value.filter((run) => run.dispatch.dispatchId !== dispatch.dispatchId)]
      .sort((left, right) => Date.parse(right.dispatch.createdAt) - Date.parse(left.dispatch.createdAt))
  }

  function upsertLatestTaskDispatch(dispatch: TaskDispatch) {
    options.latestTaskDispatchesByTaskId.value = {
      ...options.latestTaskDispatchesByTaskId.value,
      [dispatch.taskId]: dispatch,
    }
  }

  function upsertSelectedTaskRun(task: Task, dispatch: TaskDispatch) {
    if (options.selectedTaskId.value !== task.id) {
      return
    }

    replaceSelectedTaskRuns([
      { task, dispatch },
      ...options.selectedTaskRuns.value.filter((run) => run.dispatch.dispatchId !== dispatch.dispatchId),
    ])
  }

  function removeTaskRuns(taskId: string) {
    options.runs.value = options.runs.value.filter((run) => run.task.id !== taskId)
    const nextDispatches = { ...options.latestTaskDispatchesByTaskId.value }
    delete nextDispatches[taskId]
    options.latestTaskDispatchesByTaskId.value = nextDispatches

    if (options.selectedTaskId.value === taskId) {
      options.selectedTaskRuns.value = []
    }
  }

  function upsertReviewSummary(review: ReviewRecord, latestRun?: ReviewRunRecord | null) {
    const existingSummary = options.reviews.value.find((summary) => summary.review.id === review.id)
    options.reviews.value = sortReviewSummaries([
      {
        review,
        latestRun: latestRun ?? existingSummary?.latestRun,
      },
      ...options.reviews.value.filter((summary) => summary.review.id !== review.id),
    ])
  }

  function upsertLatestReviewRun(reviewId: string, latestRun: ReviewRunRecord) {
    options.reviews.value = sortReviewSummaries(
      options.reviews.value.map((summary) =>
        summary.review.id === reviewId
          ? { ...summary, latestRun }
          : summary),
    )
  }

  function upsertSelectedReviewRun(run: ReviewRunRecord) {
    if (options.selectedReviewId.value !== run.reviewId) {
      return
    }

    replaceSelectedReviewRuns([
      run,
      ...options.selectedReviewRuns.value.filter((entry) => entry.dispatchId !== run.dispatchId),
    ])
  }

  function removeReview(reviewId: string) {
    options.reviews.value = options.reviews.value.filter((summary) => summary.review.id !== reviewId)

    if (options.selectedReviewId.value === reviewId) {
      options.closeReviewDrawer()
    }
  }

  return {
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
  }
}
