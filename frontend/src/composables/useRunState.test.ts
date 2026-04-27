import { describe, expect, it } from 'vitest'
import { effectScope, ref } from 'vue'

import {
  upsertReviewRunRecord,
  upsertTaskRunRecord,
  useRunState,
} from './useRunState'
import {
  buildDispatch,
  buildReview,
  buildReviewRun,
  buildRunRecord,
  buildTask,
} from '../testing/factories'

describe('useRunState', () => {
  it('maintains task run projections and latest dispatches', () => {
    const firstTask = buildTask()
    const secondTask = buildTask({
      id: 'project-b/open/20260323-130000-another-task.md',
      project: 'project-b',
    })

    const scope = effectScope()
    const state = scope.run(() => useRunState({ tasks: ref([firstTask, secondTask]) }))

    if (!state) {
      throw new Error('Expected run state')
    }

    const olderDispatch = buildDispatch({
      dispatchId: 'dispatch-older',
      taskId: firstTask.id,
      project: firstTask.project,
      status: 'succeeded',
      createdAt: '2026-03-23T12:05:00.000Z',
      updatedAt: '2026-03-23T12:06:00.000Z',
    })
    const newerDispatch = buildDispatch({
      dispatchId: 'dispatch-newer',
      taskId: secondTask.id,
      project: secondTask.project,
      status: 'running',
      createdAt: '2026-03-23T13:05:00.000Z',
      updatedAt: '2026-03-23T13:06:00.000Z',
      finishedAt: undefined,
    })

    state.upsertRunRecord(firstTask, olderDispatch)
    state.upsertRunRecord(secondTask, newerDispatch)
    state.upsertLatestTaskDispatch(olderDispatch)
    state.upsertLatestTaskDispatch(newerDispatch)

    expect(state.activeRuns.value.map((run) => run.dispatch.dispatchId)).toEqual(['dispatch-newer'])
    expect(state.recentRuns.value.map((run) => run.dispatch.dispatchId)).toEqual([
      'dispatch-newer',
      'dispatch-older',
    ])
    expect(state.latestTaskDispatchesByTaskId.value[firstTask.id]?.dispatchId).toBe('dispatch-older')
    expect(state.latestTaskDispatchesByTaskId.value[secondTask.id]?.dispatchId).toBe('dispatch-newer')

    state.upsertRunRecord(secondTask, {
      ...newerDispatch,
      status: 'canceled',
      finishedAt: '2026-03-23T13:08:00.000Z',
      updatedAt: '2026-03-23T13:08:00.000Z',
    })
    expect(state.activeRuns.value).toEqual([])

    state.removeTaskRuns(firstTask.id)
    expect(state.runs.value.map((run) => run.task.id)).toEqual([secondTask.id])
    expect(state.latestTaskDispatchesByTaskId.value[firstTask.id]).toBeUndefined()

    const selectedHistory = upsertTaskRunRecord(
      [buildRunRecord({ ...firstTask }, { ...olderDispatch })],
      firstTask,
      {
        ...olderDispatch,
        dispatchId: 'dispatch-selected-newer',
        createdAt: '2026-03-23T14:00:00.000Z',
      },
    )
    expect(selectedHistory.map((run) => run.dispatch.dispatchId)).toEqual([
      'dispatch-selected-newer',
      'dispatch-older',
    ])

    scope.stop()
  })

  it('maintains review projections and review run history ordering', () => {
    const scope = effectScope()
    const state = scope.run(() => useRunState({ tasks: ref([]) }))

    if (!state) {
      throw new Error('Expected run state')
    }

    const activeReview = buildReview({
      id: 'review-active',
      updatedAt: '2026-03-26T12:00:00.000Z',
    })
    const finishedReview = buildReview({
      id: 'review-finished',
      updatedAt: '2026-03-26T13:00:00.000Z',
    })
    const activeRun = buildReviewRun({
      dispatchId: 'review-run-active',
      reviewId: activeReview.id,
      status: 'running',
      createdAt: '2026-03-26T12:05:00.000Z',
      updatedAt: '2026-03-26T12:06:00.000Z',
      finishedAt: undefined,
    })
    const finishedRun = buildReviewRun({
      dispatchId: 'review-run-finished',
      reviewId: finishedReview.id,
      status: 'succeeded',
      createdAt: '2026-03-26T13:05:00.000Z',
      updatedAt: '2026-03-26T13:06:00.000Z',
    })

    state.upsertReviewSummary(activeReview, activeRun)
    state.upsertReviewSummary(finishedReview, finishedRun)

    expect(state.activeReviewRuns.value.map((summary) => summary.review.id)).toEqual(['review-active'])
    expect(state.recentReviewRuns.value.map((summary) => summary.review.id)).toEqual([
      'review-finished',
      'review-active',
    ])

    const canceledRun = buildReviewRun({
      ...activeRun,
      status: 'canceled',
      finishedAt: '2026-03-26T12:08:00.000Z',
      updatedAt: '2026-03-26T12:08:00.000Z',
    })
    state.upsertLatestReviewRun(activeReview.id, canceledRun)
    expect(state.activeReviewRuns.value).toEqual([])

    const followUpRun = buildReviewRun({
      ...activeRun,
      dispatchId: 'review-run-follow-up',
      status: 'succeeded',
      createdAt: '2026-03-26T12:09:00.000Z',
      updatedAt: '2026-03-26T12:10:00.000Z',
      finishedAt: '2026-03-26T12:10:00.000Z',
    })
    const selectedHistory = upsertReviewRunRecord([activeRun], followUpRun)
    expect(selectedHistory.map((run) => run.dispatchId)).toEqual([
      'review-run-follow-up',
      'review-run-active',
    ])

    state.removeReview(finishedReview.id)
    expect(state.reviews.value.map((summary) => summary.review.id)).toEqual(['review-active'])

    scope.stop()
  })
})
