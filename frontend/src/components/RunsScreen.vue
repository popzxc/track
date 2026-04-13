<script setup lang="ts">
import RunsPage from './RunsPage.vue'
import type { RunsScreenController } from '../composables/useRunsScreenController'

const props = defineProps<{
  active: boolean
  controller: RunsScreenController
}>()

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}
</script>

<template>
  <RunsPage
    v-if="active"
    :active-review-runs="controller.activeReviewRuns.value"
    :active-runs="controller.activeRuns.value"
    :recent-review-runs="controller.recentReviewRuns.value"
    :recent-runs="controller.recentRuns.value"
    @request-open-review="controller.selectReview"
    @request-open-task-run="controller.openTaskFromRun"
    @request-open-url="openExternal"
  />
</template>
