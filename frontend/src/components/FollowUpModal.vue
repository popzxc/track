<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import type { Task, TaskDispatch } from '../types/task'

const props = defineProps<{
  busy?: boolean
  dispatch?: TaskDispatch
  open: boolean
  task: Task | null
}>()

const emit = defineEmits<{
  cancel: []
  save: [payload: { request: string }]
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

const followUpTargetLabel = computed(() => {
  if (props.dispatch?.pullRequestUrl) {
    return 'The agent will continue on the existing PR and branch.'
  }

  return 'The agent will continue on the existing branch and worktree.'
})

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
      data-testid="follow-up-modal"
      class="fixed inset-0 z-50 flex items-center justify-center bg-bg0/80 px-4 py-6"
    >
      <div class="w-full max-w-3xl border border-fg2/20 bg-bg1 p-6 shadow-panel">
        <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-4">
          <div>
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Remote follow-up
            </p>
            <h3 class="mt-2 font-display text-2xl text-fg0 sm:text-3xl">
              Continue agent work
            </h3>
            <p v-if="task" class="mt-3 text-sm leading-6 text-fg2">
              {{ task.project }} / {{ task.id }}
            </p>
            <p class="mt-3 text-sm leading-6 text-fg3">
              {{ followUpTargetLabel }}
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
            Add the next instruction for the remote agent. For example: <code>Address review comments</code>,
            <code>Rework the implementation to avoid cloning</code>, or <code>Add regression tests for the edge case</code>.
          </p>

          <label class="block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Follow-up request
            <textarea
              v-model="request"
              data-testid="follow-up-request"
              rows="8"
              class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
              placeholder="Describe what the agent should do next."
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
            data-testid="follow-up-submit"
            class="border border-aqua/35 bg-aqua/10 px-5 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:opacity-60"
            :disabled="busy || request.trim().length === 0"
            @click="submit"
          >
            {{ busy ? 'Sending...' : 'Send follow-up' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
