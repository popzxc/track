import type { Ref } from 'vue'

import {
  cancelReview,
  createReview,
  deleteReview,
  followUpReview,
} from '../api/client'
import type {
  CreateReviewInput,
  ReviewFollowUpInput,
  ReviewRecord,
  ReviewRunRecord,
} from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface UseReviewMutationsOptions {
  cancelingReviewId: Ref<string | null>
  creatingReview: Ref<boolean>
  currentPage: Ref<AppPage>
  errorMessage: Ref<string>
  followingUpReview: Ref<ReviewRecord | null>
  followingUpReviewId: Ref<string | null>
  refreshAll: () => Promise<void>
  removeReview: (reviewId: string) => void
  replaceSelectedReviewRuns: (reviewRuns: ReviewRunRecord[]) => void
  reviewPendingDeletion: Ref<ReviewRecord | null>
  saving: Ref<boolean>
  selectReview: (reviewId: string) => void
  setFriendlyError: (error: unknown) => void
  upsertLatestReviewRun: (reviewId: string, latestRun: ReviewRunRecord) => void
  upsertReviewSummary: (review: ReviewRecord, latestRun?: ReviewRunRecord | null) => void
  upsertSelectedReviewRun: (run: ReviewRunRecord) => void
}

/**
 * Owns PR review mutations and their shell-specific side effects.
 *
 * Review actions update two parallel UI surfaces: the saved review list and the
 * drawer-scoped run history. Keeping those writes together makes it obvious
 * which actions are optimistic, which ones re-sync from the backend, and when a
 * mutation should preserve the current review selection.
 */
export function useReviewMutations(options: UseReviewMutationsOptions) {
  async function createReviewFromWeb(payload: CreateReviewInput) {
    options.saving.value = true
    options.errorMessage.value = ''

    try {
      const created = await createReview(payload)
      options.creatingReview.value = false
      options.currentPage.value = 'reviews'
      options.selectReview(created.review.id)
      options.upsertReviewSummary(created.review, created.run)
      options.replaceSelectedReviewRuns([created.run])
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.saving.value = false
    }
  }

  async function confirmReviewDelete() {
    if (!options.reviewPendingDeletion.value) {
      return
    }

    options.saving.value = true
    options.errorMessage.value = ''

    try {
      const deletedReviewId = options.reviewPendingDeletion.value.id
      await deleteReview(deletedReviewId)
      options.reviewPendingDeletion.value = null
      options.removeReview(deletedReviewId)
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.saving.value = false
    }
  }

  async function cancelReviewRun(review: ReviewRecord) {
    options.cancelingReviewId.value = review.id
    options.errorMessage.value = ''

    try {
      const run = await cancelReview(review.id)
      options.upsertLatestReviewRun(review.id, run)
      options.upsertSelectedReviewRun(run)
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.cancelingReviewId.value = null
    }
  }

  async function submitReviewFollowUp(payload: ReviewFollowUpInput) {
    if (!options.followingUpReview.value) {
      return
    }

    options.followingUpReviewId.value = options.followingUpReview.value.id
    options.errorMessage.value = ''

    try {
      const review = options.followingUpReview.value
      const run = await followUpReview(review.id, payload)

      // The saved review record itself is still the durable identity. We only
      // advance its timestamp locally so the list ordering reflects the newest
      // follow-up request immediately.
      options.upsertReviewSummary(
        {
          ...review,
          updatedAt: run.createdAt,
        },
        run,
      )
      options.upsertSelectedReviewRun(run)
      options.followingUpReview.value = null
      await options.refreshAll()
    } catch (error) {
      options.setFriendlyError(error)
    } finally {
      options.followingUpReviewId.value = null
    }
  }

  return {
    cancelReviewRun,
    confirmReviewDelete,
    createReviewFromWeb,
    submitReviewFollowUp,
  }
}
