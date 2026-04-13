import { computed } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { shallowMount } from '@vue/test-utils'

import RunsScreen from './RunsScreen.vue'
import {
  buildDispatch,
  buildReview,
  buildReviewRun,
  buildTask,
} from '../testing/factories'

function createContext() {
  const task = buildTask()
  const dispatch = buildDispatch({
    taskId: task.id,
    project: task.project,
  })
  const review = buildReview()
  const latestRun = buildReviewRun({
    reviewId: review.id,
  })

  return {
    active: true,
    controller: {
      activeReviewRuns: computed(() => [{ review, latestRun }]),
      activeRuns: computed(() => [{ task, dispatch }]),
      openTaskFromRun: vi.fn(),
      recentReviewRuns: computed(() => [{ review, latestRun }]),
      recentRuns: computed(() => [{ task, dispatch }]),
      selectReview: vi.fn(),
    },
  }
}

describe('RunsScreen', () => {
  it('forwards task and review navigation requests', () => {
    const props = createContext()
    const wrapper = shallowMount(RunsScreen, {
      props,
    })

    wrapper.findComponent({ name: 'RunsPage' }).vm.$emit('request-open-review', 'review-123')
    wrapper.findComponent({ name: 'RunsPage' }).vm.$emit('request-open-task-run', props.controller.activeRuns.value[0])

    expect(props.controller.selectReview).toHaveBeenCalledWith('review-123')
    expect(props.controller.openTaskFromRun).toHaveBeenCalledWith(props.controller.activeRuns.value[0])
  })

  it('opens external URLs from the runs surface', () => {
    const openSpy = vi.spyOn(window, 'open').mockImplementation(() => null)
    const wrapper = shallowMount(RunsScreen, {
      props: createContext(),
    })

    wrapper.findComponent({ name: 'RunsPage' }).vm.$emit('request-open-url', 'https://example.com/run')

    expect(openSpy).toHaveBeenCalledWith('https://example.com/run', '_blank', 'noreferrer')
    openSpy.mockRestore()
  })
})
