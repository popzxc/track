<script setup lang="ts">
import { ref, watch } from 'vue'

import type { CreateReviewInput } from '../types/task'

const props = defineProps<{
  busy?: boolean
  mainUser?: string
  open: boolean
}>()

const emit = defineEmits<{
  cancel: []
  save: [payload: CreateReviewInput]
}>()

const pullRequestUrl = ref('')
const extraInstructions = ref('')

watch(
  () => props.open,
  (open) => {
    if (!open) {
      return
    }

    pullRequestUrl.value = ''
    extraInstructions.value = ''
  },
)

function submit() {
  emit('save', {
    pullRequestUrl: pullRequestUrl.value.trim(),
    extraInstructions: extraInstructions.value.trim() || undefined,
  })
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      data-testid="review-request-modal"
      class="fixed inset-0 z-50 flex items-center justify-center bg-bg0/80 px-4 py-6"
    >
      <div class="w-full max-w-3xl border border-fg2/20 bg-bg1 p-6 shadow-panel">
        <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-4">
          <div>
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              PR review
            </p>
            <h3 class="mt-2 font-display text-2xl text-fg0 sm:text-3xl">
              Request a review
            </h3>
            <p class="mt-3 text-sm leading-6 text-fg2">
              The agent will submit the PR review directly on GitHub, using inline comments when
              they help, and should begin the top-level review body with
              <code>@{{ mainUser || 'main-user' }} requested me to review this PR.</code>
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
            Paste a full GitHub pull request URL, then add any one-off guidance for what the review
            should pay special attention to.
          </p>

          <label class="block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Pull request URL
            <input
              v-model="pullRequestUrl"
              data-testid="review-request-url"
              type="url"
              class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
              placeholder="https://github.com/acme/project-a/pull/42"
            >
          </label>

          <label class="block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Extra instructions
            <textarea
              v-model="extraInstructions"
              data-testid="review-request-extra-instructions"
              rows="8"
              class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
              placeholder="Focus on the caching changes and whether the new tests cover the edge cases."
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
            data-testid="review-request-submit"
            class="border border-aqua/35 bg-aqua/10 px-5 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:opacity-60"
            :disabled="busy || pullRequestUrl.trim().length === 0"
            @click="submit"
          >
            {{ busy ? 'Requesting...' : 'Request review' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
