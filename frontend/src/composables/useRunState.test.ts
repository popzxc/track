import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, effectScope, ref } from 'vue'

import { useRunState } from './useRunState'
import {
  buildDispatch,
  buildReview,
  buildReviewRun,
  buildReviewSummary,
  buildRunRecord,
  buildTask,
} from '../testing/factories'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('useRunState', () => {
  it('maintains task run projections for the selected task', () => {
    const selectedTask = buildTask()
    const anotherTask = buildTask({
      id: 'project-b/open/20260323-130000-another-task.md',
      project: 'project-b',
    })

    const selectedTaskId = ref<string | null>(selectedTask.id)
    const tasks = ref([selectedTask, anotherTask])
    const selectedTaskRuns = ref([] as ReturnType<typeof buildRunRecord>[])

    const scope = effectScope()
    const state = scope.run(() =>
      useRunState({
        closeReviewDrawer: vi.fn(),
        isReviewDrawerOpen: ref(false),
        isTaskDrawerOpen: ref(true),
        latestTaskDispatchesByTaskId: ref({}),
        reviews: ref([]),
        runs: ref([]),
        selectedReview: computed(() => null),
        selectedReviewId: ref<string | null>(null),
        selectedReviewRuns: ref([]),
        selectedTask: computed(() => tasks.value.find((task) => task.id === selectedTaskId.value) ?? null),
        selectedTaskId,
        selectedTaskRuns,
        tasks,
      }),
    )

    if (!state) {
      throw new Error('Expected run state')
    }

    const firstDispatch = buildDispatch({
      dispatchId: 'dispatch-1',
      taskId: selectedTask.id,
      project: selectedTask.project,
    })
    const secondDispatch = buildDispatch({
      dispatchId: 'dispatch-2',
      taskId: selectedTask.id,
      project: selectedTask.project,
      createdAt: '2026-03-23T12:07:00.000Z',
      updatedAt: '2026-03-23T12:07:00.000Z',
    })

    state.upsertRunRecord(selectedTask, firstDispatch)
    state.upsertLatestTaskDispatch(firstDispatch)
    state.upsertSelectedTaskRun(selectedTask, firstDispatch)
    state.upsertRunRecord(selectedTask, secondDispatch)
    state.upsertSelectedTaskRun(selectedTask, secondDispatch)

    expect(state.recentRuns.value[0]?.dispatch.dispatchId).toBe('dispatch-2')
    expect(selectedTaskRuns.value.map((run) => run.dispatch.dispatchId)).toEqual(['dispatch-2', 'dispatch-1'])

    state.removeTaskRuns(selectedTask.id)

    expect(state.recentRuns.value).toEqual([])
    expect(selectedTaskRuns.value).toEqual([])

    scope.stop()
  })

  it('maintains review summaries and closes the drawer when the selected review disappears', () => {
    const closeReviewDrawer = vi.fn()
    const selectedReviewId = ref<string | null>('review-a')
    const reviews = ref([
      buildReviewSummary({
        review: {
          id: 'review-a',
          createdAt: '2026-03-23T11:00:00.000Z',
          updatedAt: '2026-03-23T12:00:00.000Z',
        },
        latestRun: {
          createdAt: '2026-03-23T12:10:00.000Z',
          updatedAt: '2026-03-23T12:10:00.000Z',
        },
      }),
    ])

    const scope = effectScope()
    const state = scope.run(() =>
      useRunState({
        closeReviewDrawer,
        isReviewDrawerOpen: ref(true),
        isTaskDrawerOpen: ref(false),
        latestTaskDispatchesByTaskId: ref({}),
        reviews,
        runs: ref([]),
        selectedReview: computed(() => reviews.value.find((summary) => summary.review.id === selectedReviewId.value)?.review ?? null),
        selectedReviewId,
        selectedReviewRuns: ref([buildReviewRun({ reviewId: 'review-a' })]),
        selectedTask: computed(() => null),
        selectedTaskId: ref<string | null>(null),
        selectedTaskRuns: ref([]),
        tasks: ref([]),
      }),
    )

    if (!state) {
      throw new Error('Expected run state')
    }

    const newReview = buildReview({
      id: 'review-b',
      updatedAt: '2026-03-23T13:00:00.000Z',
    })
    const latestRun = buildReviewRun({
      reviewId: 'review-b',
      createdAt: '2026-03-23T13:05:00.000Z',
      updatedAt: '2026-03-23T13:05:00.000Z',
    })

    state.upsertReviewSummary(newReview, latestRun)

    expect(reviews.value[0]?.review.id).toBe('review-b')

    state.upsertLatestReviewRun('review-b', buildReviewRun({
      reviewId: 'review-b',
      dispatchId: 'review-run-2',
      createdAt: '2026-03-23T13:06:00.000Z',
      updatedAt: '2026-03-23T13:06:00.000Z',
    }))
    state.upsertSelectedReviewRun(buildReviewRun({
      reviewId: 'review-a',
      dispatchId: 'review-a-run-2',
    }))
    state.removeReview('review-a')

    expect(closeReviewDrawer).toHaveBeenCalledTimes(1)

    scope.stop()
  })
})
