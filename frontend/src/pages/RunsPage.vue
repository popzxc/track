<script setup lang="ts">
import { useRouter } from 'vue-router'

import RunsPageContent from '../components/RunsPage.vue'
import { useTrackerShell } from '../composables/useTrackerShell'

const router = useRouter()
const shell = useTrackerShell()

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}

function openTaskRun(taskId: string, project: string, status: 'open' | 'closed') {
  void router.push({
    name: 'tasks',
    query: {
      task: taskId,
      project,
      closed: status === 'closed' ? '1' : undefined,
    },
  })
}

function openReview(reviewId: string) {
  void router.push({
    name: 'reviews',
    query: {
      review: reviewId,
    },
  })
}
</script>

<template>
  <RunsPageContent
    :active-review-runs="shell.activeReviewRuns.value"
    :active-runs="shell.activeRuns.value"
    :recent-review-runs="shell.recentReviewRuns.value"
    :recent-runs="shell.recentRuns.value"
    @request-open-review="openReview"
    @request-open-task-run="openTaskRun($event.task.id, $event.task.project, $event.task.status)"
    @request-open-url="openExternal"
  />
</template>
