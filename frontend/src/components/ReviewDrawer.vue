<script setup lang="ts">
import {
  dispatchBadgeClass,
  dispatchStatusLabel,
  dispatchSummary,
  formatDateTime,
} from '../features/tasks/presentation'
import type { ReviewRecord, ReviewRunRecord } from '../types/task'

const props = defineProps<{
  canCancel: boolean
  canReReview: boolean
  cancelingReviewId: string | null
  followingUpReviewId: string | null
  latestRun: ReviewRunRecord | null
  review: ReviewRecord
  reviewRuns: ReviewRunRecord[]
  saving: boolean
}>()

const emit = defineEmits<{
  close: []
  'request-cancel-review-run': [review: ReviewRecord]
  'request-delete-review': [review: ReviewRecord]
  'request-open-url': [url: string]
  'request-rereview': [review: ReviewRecord]
}>()

function remoteAgentToolLabel(tool: ReviewRecord['preferredTool'] | null | undefined): string {
  return tool === 'claude' ? 'Claude' : 'Codex'
}
</script>

<template>
  <div
    class="fixed inset-0 z-40 flex justify-end bg-bg0/70 backdrop-blur-[2px]"
    @click.self="emit('close')"
  >
    <aside
      data-testid="review-drawer"
      class="h-full w-full max-w-[980px] overflow-y-auto border-l border-fg2/20 bg-bg1 shadow-panel"
    >
      <div class="space-y-5 p-5 sm:p-6">
        <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-5">
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-fg3">
              <span>{{ review.repositoryFullName }}</span>
              <span class="text-fg3/40">/</span>
              <span>PR #{{ review.pullRequestNumber }}</span>
            </div>

            <h2 class="mt-3 whitespace-pre-wrap font-display text-3xl leading-tight text-fg0 sm:text-4xl">
              {{ review.pullRequestTitle }}
            </h2>

            <div class="mt-4 flex flex-wrap gap-2 text-[11px] font-semibold tracking-[0.08em]">
              <span class="border px-2 py-1" :class="dispatchBadgeClass(latestRun)">
                {{ dispatchStatusLabel(latestRun) }}
              </span>
              <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                via {{ remoteAgentToolLabel(review.preferredTool) }}
              </span>
              <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                @{{ review.mainUser }}
              </span>
              <span
                class="border px-2 py-1"
                :class="latestRun?.reviewSubmitted ? 'border-green/30 bg-green/10 text-green' : 'border-fg2/15 bg-bg0 text-fg2'"
              >
                {{ latestRun?.reviewSubmitted ? 'Review submitted' : 'Submission not confirmed' }}
              </span>
            </div>

            <p class="mt-4 text-sm leading-7 text-fg2">
              Created {{ formatDateTime(review.createdAt) }}
            </p>
          </div>

          <button
            type="button"
            class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0"
            @click="emit('close')"
          >
            Close
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <button
            v-if="canCancel"
            type="button"
            class="border border-orange/30 bg-orange/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-orange transition hover:bg-orange/15 disabled:opacity-60"
            :disabled="cancelingReviewId === review.id"
            @click="emit('request-cancel-review-run', review)"
          >
            {{ cancelingReviewId === review.id ? 'Canceling...' : 'Cancel review run' }}
          </button>

          <button
            v-if="canReReview"
            type="button"
            class="border border-aqua/30 bg-aqua/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:opacity-60"
            :disabled="followingUpReviewId === review.id"
            @click="emit('request-rereview', review)"
          >
            {{ followingUpReviewId === review.id ? 'Requesting...' : 'Request re-review' }}
          </button>

          <button
            type="button"
            class="border border-aqua/30 bg-aqua/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
            @click="emit('request-open-url', review.pullRequestUrl)"
          >
            View PR
          </button>

          <button
            v-if="latestRun?.githubReviewUrl"
            type="button"
            class="border border-green/30 bg-green/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-green transition hover:bg-green/15"
            @click="emit('request-open-url', latestRun.githubReviewUrl)"
          >
            View submitted review
          </button>

          <button
            type="button"
            class="border border-red/30 bg-red/10 px-4 py-2.5 text-sm font-semibold tracking-[0.08em] text-red transition hover:bg-red/15 disabled:opacity-60"
            :disabled="saving"
            @click="emit('request-delete-review', review)"
          >
            Delete review
          </button>
        </div>

        <section class="border border-fg2/15 bg-bg0/60 p-4">
          <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Latest status
          </p>
          <p class="mt-4 text-sm leading-7 text-fg1">
            {{ dispatchSummary(latestRun, 'review') }}
          </p>
          <p class="mt-4 text-xs leading-6 text-fg3">
            The actual review discussion lives on GitHub, including any inline comments the agent submitted.
          </p>
          <dl class="mt-4 grid gap-4 text-sm md:grid-cols-2 xl:grid-cols-3">
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                Pull request
              </dt>
              <dd class="mt-1 break-all text-fg1">
                {{ review.pullRequestUrl }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                Base branch
              </dt>
              <dd class="mt-1 text-fg1">
                {{ review.baseBranch }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                Workspace key
              </dt>
              <dd class="mt-1 break-all text-fg1">
                {{ review.workspaceKey }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                Review tool
              </dt>
              <dd class="mt-1 text-fg1">
                {{ remoteAgentToolLabel(review.preferredTool) }}
              </dd>
            </div>
            <div v-if="latestRun?.targetHeadOid">
              <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                Pinned commit
              </dt>
              <dd class="mt-1 break-all text-fg1">
                {{ latestRun.targetHeadOid }}
              </dd>
            </div>
            <div v-if="latestRun?.githubReviewUrl">
              <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                Submitted review
              </dt>
              <dd class="mt-1 break-all text-fg1">
                {{ latestRun.githubReviewUrl }}
              </dd>
            </div>
          </dl>
        </section>

        <section class="grid gap-4 xl:grid-cols-2">
          <section class="border border-fg2/15 bg-bg0/60 p-4">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Default review prompt
            </p>
            <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
              {{ review.defaultReviewPrompt || 'No default review prompt was saved for this review.' }}
            </div>
          </section>

          <section class="border border-fg2/15 bg-bg0/60 p-4">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Extra instructions
            </p>
            <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
              {{ review.extraInstructions || 'No extra instructions were provided for this review.' }}
            </div>
          </section>
        </section>

        <section class="border border-fg2/15 bg-bg0/60 p-4">
          <div class="flex items-center justify-between gap-3">
            <div>
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Review run history
              </p>
              <p class="mt-2 text-sm text-fg2">
                Each re-review adds another run here so you can compare requests, commits, and outcomes over time.
              </p>
            </div>
            <span class="text-xs text-fg3">{{ reviewRuns.length }}</span>
          </div>

          <div
            v-if="reviewRuns.length === 0"
            class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
          >
            This review has no run history yet.
          </div>

          <div v-else class="mt-4 space-y-3">
            <article
              v-for="(run, index) in reviewRuns"
              :key="run.dispatchId"
              class="border border-fg2/15 bg-bg1/70 p-4"
            >
              <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                    <span
                      v-if="index === 0"
                      class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2"
                    >
                      Latest
                    </span>
                    <span class="border px-2 py-1" :class="dispatchBadgeClass(run)">
                      {{ dispatchStatusLabel(run) }}
                    </span>
                    <span class="text-fg3">Started {{ formatDateTime(run.createdAt) }}</span>
                    <span v-if="run.followUpRequest" class="text-fg3">• Re-review</span>
                  </div>
                </div>

                <button
                  v-if="run.githubReviewUrl"
                  type="button"
                  class="border border-green/30 bg-green/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-green transition hover:bg-green/15"
                  @click="emit('request-open-url', run.githubReviewUrl)"
                >
                  View review
                </button>
              </div>

              <p class="mt-4 text-sm leading-7 text-fg1">
                {{ dispatchSummary(run, 'review') }}
              </p>

              <dl class="mt-4 grid gap-4 text-sm md:grid-cols-2 xl:grid-cols-3">
                <div v-if="run.finishedAt">
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Finished
                  </dt>
                  <dd class="mt-1 text-fg1">
                    {{ formatDateTime(run.finishedAt) }}
                  </dd>
                </div>
                <div v-if="run.branchName">
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Branch
                  </dt>
                  <dd class="mt-1 break-all text-fg1">
                    {{ run.branchName }}
                  </dd>
                </div>
                <div v-if="run.worktreePath">
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Worktree
                  </dt>
                  <dd class="mt-1 break-all text-fg1">
                    {{ run.worktreePath }}
                  </dd>
                </div>
                <div v-if="run.targetHeadOid">
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Pinned commit
                  </dt>
                  <dd class="mt-1 break-all text-fg1">
                    {{ run.targetHeadOid }}
                  </dd>
                </div>
              </dl>

              <details
                v-if="run.followUpRequest"
                class="mt-4 border border-aqua/20 bg-aqua/6 p-4"
              >
                <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-aqua">
                  Re-review request
                </summary>
                <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                  {{ run.followUpRequest }}
                </div>
              </details>

              <details
                v-if="run.notes"
                class="mt-4 border border-fg2/15 bg-bg0/70 p-4"
              >
                <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Run notes
                </summary>
                <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                  {{ run.notes }}
                </div>
              </details>

              <details
                v-if="run.errorMessage"
                class="mt-4 border border-red/20 bg-red/5 p-4"
              >
                <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-red">
                  Error details
                </summary>
                <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-red">
                  {{ run.errorMessage }}
                </div>
              </details>
            </article>
          </div>
        </section>
      </div>
    </aside>
  </div>
</template>
