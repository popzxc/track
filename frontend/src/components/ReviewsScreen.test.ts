import { computed, nextTick, ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { shallowMount } from '@vue/test-utils'

import ReviewsScreen from './ReviewsScreen.vue'
import {
  buildRemoteAgentSettings,
  buildReview,
  buildReviewRun,
} from '../testing/factories'

function createContext() {
  const review = buildReview()
  const latestRun = buildReviewRun({ reviewId: review.id })

  const creatingReview = ref(false)
  const followingUpReview = ref<ReturnType<typeof buildReview> | null>(null)
  const reviewPendingDeletion = ref<ReturnType<typeof buildReview> | null>(null)

  return {
    active: true,
    controller: {
      cancelingReviewId: ref<string | null>(null),
      canRequestReview: computed(() => true),
      closeReviewDrawer: vi.fn(),
      creatingReview,
      currentPage: ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('reviews'),
      defaultRemoteAgentPreferredTool: computed(() => 'codex' as const),
      errorMessage: ref(''),
      followingUpReview,
      followingUpReviewId: ref<string | null>(null),
      isReviewDrawerOpen: ref(true),
      refreshAll: vi.fn().mockResolvedValue(undefined),
      remoteAgentSettings: ref(buildRemoteAgentSettings()),
      removeReview: vi.fn(),
      replaceSelectedReviewRuns: vi.fn(),
      reviewPendingDeletion,
      reviewRequestDisabledReason: computed(() => undefined),
      reviews: ref([{ review, latestRun }]),
      saving: ref(false),
      selectedReview: computed(() => review),
      selectedReviewCanCancel: computed(() => true),
      selectedReviewCanReReview: computed(() => true),
      selectedReviewLatestRun: computed(() => latestRun),
      selectedReviewRuns: ref([latestRun]),
      selectReview: vi.fn(),
      setFriendlyError: vi.fn(),
      upsertLatestReviewRun: vi.fn(),
      upsertReviewSummary: vi.fn(),
      upsertSelectedReviewRun: vi.fn(),
    },
  }
}

describe('ReviewsScreen', () => {
  it('opens the review request modal from the page surface', async () => {
    const wrapper = shallowMount(ReviewsScreen, {
      props: createContext(),
    })

    wrapper.findComponent({ name: 'ReviewsPage' }).vm.$emit('request-create-review')
    await nextTick()

    expect(wrapper.findComponent({ name: 'ReviewRequestModal' }).props('open')).toBe(true)
  })

  it('opens the review delete confirmation from the drawer', async () => {
    const wrapper = shallowMount(ReviewsScreen, {
      props: createContext(),
    })

    wrapper.findComponent({ name: 'ReviewDrawer' }).vm.$emit('request-delete-review', createContext().controller.selectedReview.value)
    await nextTick()

    expect(wrapper.findComponent({ name: 'ConfirmDialog' }).props('open')).toBe(true)
  })
})
