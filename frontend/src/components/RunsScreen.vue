<script setup lang="ts">
import type { ComputedRef } from 'vue'

import RunsPage from './RunsPage.vue'
import type {
  ReviewSummary,
  RunRecord,
} from '../types/task'

interface RunsScreenContext {
  activeReviewRuns: ComputedRef<ReviewSummary[]>
  activeRuns: ComputedRef<RunRecord[]>
  openTaskFromRun: (run: RunRecord) => void
  recentReviewRuns: ComputedRef<ReviewSummary[]>
  recentRuns: ComputedRef<RunRecord[]>
  selectReview: (reviewId: string) => void
}

const props = defineProps<{
  active: boolean
  context: RunsScreenContext
}>()

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}
</script>

<template>
  <RunsPage
    v-if="active"
    :active-review-runs="context.activeReviewRuns.value"
    :active-runs="context.activeRuns.value"
    :recent-review-runs="context.recentReviewRuns.value"
    :recent-runs="context.recentRuns.value"
    @request-open-review="context.selectReview"
    @request-open-task-run="context.openTaskFromRun"
    @request-open-url="openExternal"
  />
</template>
