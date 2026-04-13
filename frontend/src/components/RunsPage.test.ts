import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import RunsPage from './RunsPage.vue'
import { buildReviewSummary, buildRunRecord } from '../testing/factories'

describe('RunsPage', () => {
  it('emits run page actions for task and review entries', async () => {
    const activeRun = buildRunRecord()
    const recentRun = buildRunRecord(
      { id: 'project-a/open/20260323-130000-follow-up.md', description: 'Follow up task' },
      { dispatchId: 'dispatch-999' },
    )
    const activeReview = buildReviewSummary({
      latestRun: {
        dispatchId: 'review-dispatch-active',
        status: 'running',
        finishedAt: undefined,
        reviewSubmitted: false,
      },
    })
    const recentReview = buildReviewSummary({
      review: {
        id: 'review-recent',
        pullRequestTitle: 'Recent review',
      },
      latestRun: {
        dispatchId: 'review-dispatch-recent',
        reviewSubmitted: true,
      },
    })

    const wrapper = mount(RunsPage, {
      props: {
        activeReviewRuns: [activeReview],
        activeRuns: [activeRun],
        recentReviewRuns: [recentReview],
        recentRuns: [recentRun],
      },
    })

    await wrapper.get('[data-testid="active-task-open-button"]').trigger('click')
    await wrapper.get('[data-testid="active-review-open-button"]').trigger('click')
    await wrapper.get('[data-testid="recent-run-view-pr-button"]').trigger('click')
    await wrapper.get('[data-testid="recent-review-view-pr-button"]').trigger('click')

    expect(wrapper.emitted('request-open-task-run')).toEqual([[activeRun]])
    expect(wrapper.emitted('request-open-review')).toEqual([[activeReview.review.id]])
    expect(wrapper.emitted('request-open-url')).toEqual([
      [recentRun.dispatch.pullRequestUrl],
      [recentReview.review.pullRequestUrl],
    ])
    expect(wrapper.text()).toContain('Review submitted')
  })

  it('shows empty states when no runs exist', () => {
    const wrapper = mount(RunsPage, {
      props: {
        activeReviewRuns: [],
        activeRuns: [],
        recentReviewRuns: [],
        recentRuns: [],
      },
    })

    expect(wrapper.text()).toContain('No task runs are active at the moment.')
    expect(wrapper.text()).toContain('No PR reviews are running right now.')
    expect(wrapper.text()).toContain('No dispatch history has been recorded yet.')
    expect(wrapper.text()).toContain('No PR review history has been recorded yet.')
  })
})
