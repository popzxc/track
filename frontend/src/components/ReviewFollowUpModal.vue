<script setup lang="ts">
import { ref, watch } from 'vue'

import type { ReviewFollowUpInput, ReviewRecord } from '../types/task'

const props = defineProps<{
  busy?: boolean
  open: boolean
  review: ReviewRecord | null
}>()

const emit = defineEmits<{
  cancel: []
  save: [payload: ReviewFollowUpInput]
}>()

const request = ref('')

watch(
  () => props.open,
  (open) => {
    if (open) {
      request.value = ''
    }
  },
)

function submit() {
  emit('save', {
    request: request.value.trim(),
  })
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      data-testid="review-follow-up-modal"
      class="fixed inset-0 z-50 flex items-center justify-center bg-bg0/80 px-4 py-6"
    >
      <div class="w-full max-w-3xl border border-fg2/20 bg-bg1 p-6 shadow-panel">
        <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-4">
          <div>
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              PR re-review
            </p>
            <h3 class="mt-2 font-display text-2xl text-fg0 sm:text-3xl">
              Request a re-review
            </h3>
            <p v-if="review" class="mt-3 text-sm leading-6 text-fg2">
              {{ review.repositoryFullName }} / PR #{{ review.pullRequestNumber }}
            </p>
            <p v-if="review" class="mt-3 break-all text-sm leading-6 text-fg3">
              {{ review.pullRequestUrl }}
            </p>
          </div>
          <button
            type="button"
            class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/45 hover:text-fg0"
            @click="emit('cancel')"
          >
            Close
          </button>
        </div>

        <div class="mt-5 space-y-4">
          <p class="text-sm leading-7 text-fg2">
            Tell the agent what to check in this next pass. For example:
            <code>Verify that the comments I marked as valid are fixed</code>,
            <code>Focus on the new tests and edge cases</code>, or
            <code>Ignore the docs-only changes and look for behavior regressions</code>.
          </p>

          <label class="block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Re-review request
            <textarea
              v-model="request"
              data-testid="review-follow-up-request"
              rows="8"
              class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
              placeholder="Describe what the agent should pay attention to in the next review."
            />
          </label>
        </div>

        <div class="mt-6 flex justify-end gap-3">
          <button
            type="button"
            class="border border-fg2/20 bg-bg0 px-4 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/45 hover:text-fg0"
            @click="emit('cancel')"
          >
            Cancel
          </button>
          <button
            type="button"
            data-testid="review-follow-up-submit"
            class="border border-aqua/35 bg-aqua/10 px-5 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:opacity-60"
            :disabled="busy || request.trim().length === 0"
            @click="submit"
          >
            {{ busy ? 'Requesting...' : 'Request re-review' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
