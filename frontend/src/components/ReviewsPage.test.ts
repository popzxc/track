import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import ReviewsPage from './ReviewsPage.vue'
import { buildReviewSummary } from '../testing/factories'

describe('ReviewsPage', () => {
  it('emits review page actions and renders persisted review summaries', async () => {
    const reviewSummary = buildReviewSummary()

    const wrapper = mount(ReviewsPage, {
      props: {
        canRequestReview: false,
        reviewRequestDisabledReason: 'Set the main GitHub user in Settings to enable PR reviews.',
        reviews: [reviewSummary],
      },
    })

    expect(wrapper.get('[data-testid="request-review-button"]').attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('Review submitted')

    await wrapper.get('[data-testid="open-review-settings-button"]').trigger('click')
    await wrapper.get('[data-testid="review-row"]').trigger('click')

    expect(wrapper.emitted('request-open-settings')).toEqual([[]])
    expect(wrapper.emitted('request-select-review')).toEqual([[reviewSummary.review.id]])
  })

  it('shows the empty review state when no reviews exist', () => {
    const wrapper = mount(ReviewsPage, {
      props: {
        canRequestReview: true,
        reviews: [],
      },
    })

    expect(wrapper.text()).toContain('No PR reviews yet.')
    expect(wrapper.findAll('[data-testid="review-row"]')).toHaveLength(0)
  })
})
