import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import ReviewDrawer from './ReviewDrawer.vue'
import { buildReview, buildReviewRun } from '../testing/factories'

describe('ReviewDrawer', () => {
  it('renders review context and emits drawer actions', async () => {
    const review = buildReview({ preferredTool: 'claude' })
    const latestRun = buildReviewRun({
      reviewId: review.id,
      githubReviewUrl: 'https://github.com/acme/project-a/pull/42#pullrequestreview-1001',
      targetHeadOid: 'def456abc789',
      followUpRequest: 'Check the latest queue changes.',
    })

    const wrapper = mount(ReviewDrawer, {
      props: {
        canCancel: true,
        canReReview: true,
        cancelingReviewId: null,
        followingUpReviewId: null,
        latestRun,
        review,
        reviewRuns: [latestRun],
        saving: false,
      },
    })

    expect(wrapper.get('[data-testid="review-drawer"]').text()).toContain(review.pullRequestTitle)
    expect(wrapper.text()).toContain('via Claude')
    expect(wrapper.text()).toContain('Pinned commit')
    expect(wrapper.text()).toContain('def456abc789')
    expect(wrapper.text()).toContain('Re-review request')

    await wrapper.findAll('button').find((button) => button.text().includes('Cancel review run'))?.trigger('click')
    await wrapper.findAll('button').find((button) => button.text().includes('Request re-review'))?.trigger('click')
    await wrapper.findAll('button').find((button) => button.text().includes('View PR'))?.trigger('click')
    await wrapper.findAll('button').find((button) => button.text().includes('View submitted review'))?.trigger('click')
    await wrapper.findAll('button').find((button) => button.text().includes('Delete review'))?.trigger('click')
    await wrapper.findAll('button').find((button) => button.text().includes('Close'))?.trigger('click')

    expect(wrapper.emitted('request-cancel-review-run')).toEqual([[review]])
    expect(wrapper.emitted('request-rereview')).toEqual([[review]])
    expect(wrapper.emitted('request-open-url')).toEqual([
      [review.pullRequestUrl],
      [latestRun.githubReviewUrl],
    ])
    expect(wrapper.emitted('request-delete-review')).toEqual([[review]])
    expect(wrapper.emitted('close')).toEqual([[]])
  })

  it('shows the empty history state when no review runs were recorded', () => {
    const review = buildReview()

    const wrapper = mount(ReviewDrawer, {
      props: {
        canCancel: false,
        canReReview: false,
        cancelingReviewId: null,
        followingUpReviewId: null,
        latestRun: null,
        review,
        reviewRuns: [],
        saving: false,
      },
    })

    expect(wrapper.text()).toContain('This review has no run history yet.')
    expect(wrapper.text()).toContain('No PR review run has been recorded yet.')
  })
})
