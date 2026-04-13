<script setup lang="ts">
import {
  dispatchBadgeClass,
  dispatchStatusLabel,
  dispatchSummary,
  formatDateTime,
} from '../features/tasks/presentation'
import type { ReviewSummary } from '../types/task'

const props = defineProps<{
  canRequestReview: boolean
  reviewRequestDisabledReason?: string
  reviews: ReviewSummary[]
}>()

const emit = defineEmits<{
  'request-create-review': []
  'request-open-settings': []
  'request-select-review': [reviewId: string]
}>()
</script>

<template>
  <section class="space-y-4">
    <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <div class="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
        <div>
          <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
            Reviews
          </h1>
          <p class="mt-2 text-sm text-fg3">
            Standalone PR reviews with persisted history and cleanup.
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-3">
          <button
            type="button"
            data-testid="request-review-button"
            class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="!canRequestReview"
            @click="emit('request-create-review')"
          >
            Request review
          </button>
          <button
            v-if="reviewRequestDisabledReason"
            type="button"
            data-testid="open-review-settings-button"
            class="border border-fg2/20 bg-bg0 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
            @click="emit('request-open-settings')"
          >
            Open settings
          </button>
        </div>
      </div>
    </div>

    <div
      v-if="reviewRequestDisabledReason"
      class="border border-yellow/25 bg-yellow/8 px-4 py-3 text-sm leading-6 text-yellow shadow-panel"
    >
      {{ reviewRequestDisabledReason }}
    </div>

    <div v-if="reviews.length === 0" class="border border-fg2/20 bg-bg1/95 px-4 py-12 text-center shadow-panel">
      <p class="font-display text-2xl text-fg0">
        No PR reviews yet.
      </p>
      <p class="mt-3 text-sm leading-6 text-fg2">
        Request a review from a GitHub PR URL and it will show up here with its run history.
      </p>
    </div>

    <div v-else class="space-y-4">
      <article
        v-for="summary in reviews"
        :key="summary.review.id"
        :data-review-id="summary.review.id"
        class="border border-fg2/20 bg-bg1/95 shadow-panel transition hover:border-fg1/25"
      >
        <button
          type="button"
          data-testid="review-row"
          class="w-full px-4 py-5 text-left transition hover:bg-bg0/35"
          @click="emit('request-select-review', summary.review.id)"
        >
          <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div class="min-w-0">
              <p class="text-xs tracking-[0.08em] text-fg3">
                {{ summary.review.repositoryFullName }} / PR #{{ summary.review.pullRequestNumber }}
              </p>
              <h2 class="mt-3 whitespace-pre-wrap text-xl leading-8 text-fg0">
                {{ summary.review.pullRequestTitle }}
              </h2>
              <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                <span class="border px-2 py-1" :class="dispatchBadgeClass(summary.latestRun)">
                  {{ dispatchStatusLabel(summary.latestRun) }}
                </span>
                <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                  @{{ summary.review.mainUser }}
                </span>
                <span class="text-fg3">Created {{ formatDateTime(summary.review.createdAt) }}</span>
                <span v-if="summary.latestRun?.reviewSubmitted" class="text-green">
                  Review submitted
                </span>
              </div>
              <p class="mt-4 text-sm leading-7 text-fg2">
                {{ dispatchSummary(summary.latestRun, 'review') }}
              </p>
            </div>
          </div>
        </button>
      </article>
    </div>
  </section>
</template>
