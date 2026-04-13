<script setup lang="ts">
import {
  dispatchBadgeClass,
  dispatchStatusLabel,
  dispatchSummary,
  formatDateTime,
} from '../features/tasks/presentation'
import { taskTitle } from '../features/tasks/description'
import type { ReviewSummary, RunRecord } from '../types/task'

const props = defineProps<{
  activeReviewRuns: ReviewSummary[]
  activeRuns: RunRecord[]
  recentReviewRuns: ReviewSummary[]
  recentRuns: RunRecord[]
}>()

const emit = defineEmits<{
  'request-open-review': [reviewId: string]
  'request-open-task-run': [run: RunRecord]
  'request-open-url': [url: string]
}>()
</script>

<template>
  <section class="space-y-4">
    <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
        Runs
      </h1>
      <p class="mt-2 text-sm text-fg3">
        Active agents and recent outcomes
      </p>
    </div>

    <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <div class="flex items-center justify-between gap-3">
        <div>
          <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Active task runs
          </p>
          <p class="mt-2 text-sm text-fg2">
            Task agents that are still preparing or actively running.
          </p>
        </div>
        <span class="text-xs text-fg3">{{ activeRuns.length }}</span>
      </div>

      <div
        v-if="activeRuns.length === 0"
        class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
      >
        No task runs are active at the moment.
      </div>

      <div v-else class="mt-4 space-y-3">
        <article
          v-for="run in activeRuns"
          :key="run.dispatch.dispatchId"
          class="border border-fg2/15 bg-bg0/60 p-4"
        >
          <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div class="min-w-0">
              <p class="text-xs tracking-[0.08em] text-fg3">
                {{ run.task.project }}
              </p>
              <h2 class="mt-3 whitespace-pre-wrap text-xl leading-8 text-fg0">
                {{ taskTitle(run.task) }}
              </h2>
              <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                <span class="border px-2 py-1" :class="dispatchBadgeClass(run.dispatch)">
                  {{ dispatchStatusLabel(run.dispatch) }}
                </span>
                <span class="text-fg3">Started {{ formatDateTime(run.dispatch.createdAt) }}</span>
              </div>
              <p class="mt-4 text-sm leading-7 text-fg2">
                {{ dispatchSummary(run.dispatch) }}
              </p>
            </div>

            <div class="flex shrink-0 flex-wrap gap-2">
              <button
                type="button"
                data-testid="active-task-open-button"
                class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                @click="emit('request-open-task-run', run)"
              >
                Open task
              </button>
              <button
                v-if="run.dispatch.pullRequestUrl"
                type="button"
                data-testid="active-task-view-pr-button"
                class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                @click="emit('request-open-url', run.dispatch.pullRequestUrl)"
              >
                View PR
              </button>
            </div>
          </div>
        </article>
      </div>
    </section>

    <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <div class="flex items-center justify-between gap-3">
        <div>
          <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Active PR reviews
          </p>
          <p class="mt-2 text-sm text-fg2">
            Standalone review runs that are still preparing or actively running.
          </p>
        </div>
        <span class="text-xs text-fg3">{{ activeReviewRuns.length }}</span>
      </div>

      <div
        v-if="activeReviewRuns.length === 0"
        class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
      >
        No PR reviews are running right now.
      </div>

      <div v-else class="mt-4 space-y-3">
        <article
          v-for="summary in activeReviewRuns"
          :key="summary.latestRun?.dispatchId ?? summary.review.id"
          class="border border-fg2/15 bg-bg0/60 p-4"
        >
          <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div class="min-w-0">
              <p class="text-xs tracking-[0.08em] text-fg3">
                {{ summary.review.repositoryFullName }}
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
                <span class="text-fg3">
                  Started {{ formatDateTime(summary.latestRun?.createdAt ?? summary.review.createdAt) }}
                </span>
              </div>
              <p class="mt-4 text-sm leading-7 text-fg2">
                {{ dispatchSummary(summary.latestRun, 'review') }}
              </p>
            </div>

            <div class="flex shrink-0 flex-wrap gap-2">
              <button
                type="button"
                data-testid="active-review-open-button"
                class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                @click="emit('request-open-review', summary.review.id)"
              >
                Open review
              </button>
              <button
                type="button"
                data-testid="active-review-view-pr-button"
                class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                @click="emit('request-open-url', summary.review.pullRequestUrl)"
              >
                View PR
              </button>
            </div>
          </div>
        </article>
      </div>
    </section>

    <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <div class="flex items-center justify-between gap-3">
        <div>
          <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Recent runs
          </p>
          <p class="mt-2 text-sm text-fg2">
            The latest dispatch results across all tasks, including follow-ups and failures.
          </p>
        </div>
        <span class="text-xs text-fg3">{{ recentRuns.length }}</span>
      </div>

      <div
        v-if="recentRuns.length === 0"
        class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
      >
        No dispatch history has been recorded yet.
      </div>

      <div v-else class="mt-4 space-y-3">
        <article
          v-for="run in recentRuns"
          :key="run.dispatch.dispatchId"
          class="border border-fg2/15 bg-bg0/60 p-4"
        >
          <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div class="min-w-0">
              <p class="text-xs tracking-[0.08em] text-fg3">
                {{ run.task.project }}
              </p>
              <h2 class="mt-3 whitespace-pre-wrap text-lg leading-8 text-fg0">
                {{ taskTitle(run.task) }}
              </h2>
              <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                <span class="border px-2 py-1" :class="dispatchBadgeClass(run.dispatch)">
                  {{ dispatchStatusLabel(run.dispatch) }}
                </span>
                <span class="text-fg3">Started {{ formatDateTime(run.dispatch.createdAt) }}</span>
                <span v-if="run.dispatch.finishedAt" class="text-fg3">
                  • Finished {{ formatDateTime(run.dispatch.finishedAt) }}
                </span>
              </div>
              <p class="mt-4 text-sm leading-7 text-fg2">
                {{ dispatchSummary(run.dispatch) }}
              </p>
            </div>

            <div class="flex shrink-0 flex-wrap gap-2">
              <button
                type="button"
                data-testid="recent-run-open-button"
                class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                @click="emit('request-open-task-run', run)"
              >
                Open task
              </button>
              <button
                v-if="run.dispatch.pullRequestUrl"
                type="button"
                data-testid="recent-run-view-pr-button"
                class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                @click="emit('request-open-url', run.dispatch.pullRequestUrl)"
              >
                View PR
              </button>
            </div>
          </div>
        </article>
      </div>
    </section>

    <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <div class="flex items-center justify-between gap-3">
        <div>
          <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Recent PR reviews
          </p>
          <p class="mt-2 text-sm text-fg2">
            The latest standalone review outcomes, including submitted reviews and failures.
          </p>
        </div>
        <span class="text-xs text-fg3">{{ recentReviewRuns.length }}</span>
      </div>

      <div
        v-if="recentReviewRuns.length === 0"
        class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
      >
        No PR review history has been recorded yet.
      </div>

      <div v-else class="mt-4 space-y-3">
        <article
          v-for="summary in recentReviewRuns"
          :key="summary.latestRun?.dispatchId ?? summary.review.id"
          class="border border-fg2/15 bg-bg0/60 p-4"
        >
          <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div class="min-w-0">
              <p class="text-xs tracking-[0.08em] text-fg3">
                {{ summary.review.repositoryFullName }}
              </p>
              <h2 class="mt-3 whitespace-pre-wrap text-lg leading-8 text-fg0">
                {{ summary.review.pullRequestTitle }}
              </h2>
              <div class="mt-3 flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                <span class="border px-2 py-1" :class="dispatchBadgeClass(summary.latestRun)">
                  {{ dispatchStatusLabel(summary.latestRun) }}
                </span>
                <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                  @{{ summary.review.mainUser }}
                </span>
                <span class="text-fg3">
                  Started {{ formatDateTime(summary.latestRun?.createdAt ?? summary.review.createdAt) }}
                </span>
                <span v-if="summary.latestRun?.finishedAt" class="text-fg3">
                  • Finished {{ formatDateTime(summary.latestRun?.finishedAt) }}
                </span>
                <span v-if="summary.latestRun?.reviewSubmitted" class="text-green">
                  Review submitted
                </span>
              </div>
              <p class="mt-4 text-sm leading-7 text-fg2">
                {{ dispatchSummary(summary.latestRun, 'review') }}
              </p>
            </div>

            <div class="flex shrink-0 flex-wrap gap-2">
              <button
                type="button"
                data-testid="recent-review-open-button"
                class="border border-fg2/20 bg-bg0 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0"
                @click="emit('request-open-review', summary.review.id)"
              >
                Open review
              </button>
              <button
                type="button"
                data-testid="recent-review-view-pr-button"
                class="border border-aqua/30 bg-aqua/10 px-4 py-3 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                @click="emit('request-open-url', summary.review.pullRequestUrl)"
              >
                View PR
              </button>
            </div>
          </div>
        </article>
      </div>
    </section>
  </section>
</template>
