import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, defineComponent, nextTick, ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'

import { useBackgroundSync } from './useBackgroundSync'
import { buildRunRecord } from '../testing/factories'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('useBackgroundSync', () => {
  it('refreshes queue data when the visible task filters change', async () => {
    const loadTasks = vi.fn(async () => undefined)
    const loadLatestDispatchesForVisibleTasks = vi.fn(async () => undefined)
    const loadSelectedTaskRunHistory = vi.fn(async () => undefined)

    const selectedProjectFilter = ref('')
    const showClosed = ref(false)

    const state: Record<string, unknown> = {}
    const wrapper = mount(defineComponent({
      setup() {
        Object.assign(state, useBackgroundSync({
          activeReviewRuns: computed(() => []),
          activeRuns: computed(() => []),
          cancelingDispatchTaskId: ref(null),
          cancelingReviewId: ref(null),
          dispatchingTaskId: ref(null),
          discardingDispatchTaskId: ref(null),
          followingUpTaskId: ref(null),
          isReviewDrawerOpen: ref(false),
          isTaskDrawerOpen: ref(false),
          loading: ref(false),
          loadLatestDispatchesForVisibleTasks,
          loadReviews: vi.fn(async () => undefined),
          loadRuns: vi.fn(async () => undefined),
          loadSelectedReviewRunHistory: vi.fn(async () => undefined),
          loadSelectedTaskRunHistory,
          loadTasks,
          refreshAll: vi.fn(async () => undefined),
          refreshing: ref(false),
          saving: ref(false),
          selectedProjectFilter,
          selectedReview: computed(() => null),
          selectedReviewRuns: ref([]),
          selectedTask: computed(() => null),
          selectedTaskRuns: ref([]),
          setFriendlyError: vi.fn(),
          showClosed,
        }))

        return () => null
      },
    }))

    selectedProjectFilter.value = 'project-a'
    await nextTick()
    await flushPromises()

    expect(loadTasks).toHaveBeenCalledTimes(1)
    expect(loadLatestDispatchesForVisibleTasks).toHaveBeenCalledTimes(1)
    expect(loadSelectedTaskRunHistory).toHaveBeenCalledTimes(1)

    wrapper.unmount()
  })

  it('polls for task and run updates using the shell refresh rules', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch')
    fetchMock
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ version: 1 }),
      } as Response)
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ version: 2 }),
      } as Response)

    const refreshAll = vi.fn(async () => undefined)
    const loadRuns = vi.fn(async () => undefined)
    const loadReviews = vi.fn(async () => undefined)
    const loadLatestDispatchesForVisibleTasks = vi.fn(async () => undefined)

    const state: { backgroundSync?: ReturnType<typeof useBackgroundSync> } = {}
    const wrapper = mount(defineComponent({
      setup() {
        state.backgroundSync = useBackgroundSync({
          activeReviewRuns: computed(() => []),
          activeRuns: computed(() => [buildRunRecord()]),
          cancelingDispatchTaskId: ref(null),
          cancelingReviewId: ref(null),
          dispatchingTaskId: ref(null),
          discardingDispatchTaskId: ref(null),
          followingUpTaskId: ref(null),
          isReviewDrawerOpen: ref(false),
          isTaskDrawerOpen: ref(false),
          loading: ref(false),
          loadLatestDispatchesForVisibleTasks,
          loadReviews,
          loadRuns,
          loadSelectedReviewRunHistory: vi.fn(async () => undefined),
          loadSelectedTaskRunHistory: vi.fn(async () => undefined),
          loadTasks: vi.fn(async () => undefined),
          refreshAll,
          refreshing: ref(false),
          saving: ref(false),
          selectedProjectFilter: ref(''),
          selectedReview: computed(() => null),
          selectedReviewRuns: ref([]),
          selectedTask: computed(() => null),
          selectedTaskRuns: ref([]),
          setFriendlyError: vi.fn(),
          showClosed: ref(false),
        })

        return () => null
      },
    }))

    if (!state.backgroundSync) {
      throw new Error('Expected background sync state')
    }
    const backgroundSync = state.backgroundSync

    refreshAll.mockClear()

    await backgroundSync.syncTaskChangeVersion()
    await backgroundSync.pollForTaskChanges()

    expect(refreshAll).toHaveBeenCalledTimes(1)

    await backgroundSync.pollForRunChanges()

    expect(loadRuns).toHaveBeenCalledTimes(1)
    expect(loadReviews).toHaveBeenCalledTimes(1)
    expect(loadLatestDispatchesForVisibleTasks).toHaveBeenCalledTimes(1)

    wrapper.unmount()
  })
})
