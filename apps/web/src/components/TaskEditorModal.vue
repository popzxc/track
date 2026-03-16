<script setup lang="ts">
import { ref, watch } from 'vue'

import type { Priority, Task } from '../types/task'

const props = defineProps<{
  busy?: boolean
  open: boolean
  task: Task | null
}>()

const emit = defineEmits<{
  cancel: []
  save: [payload: { description: string; priority: Priority }]
}>()

const description = ref('')
const priority = ref<Priority>('medium')

watch(
  () => props.task,
  (task) => {
    description.value = task?.description ?? ''
    priority.value = task?.priority ?? 'medium'
  },
  { immediate: true },
)

function submit() {
  emit('save', {
    description: description.value.trim(),
    priority: priority.value,
  })
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-ink/35 px-4 backdrop-blur-sm"
    >
      <div class="w-full max-w-2xl rounded-[32px] border border-white/70 bg-white/95 p-6 shadow-panel">
        <div class="flex items-start justify-between gap-4">
          <div>
            <p class="text-xs uppercase tracking-[0.24em] text-copper/80">
              Edit task
            </p>
            <h3 class="mt-2 font-display text-3xl text-ink">
              Refine the task details
            </h3>
          </div>
          <button
            type="button"
            class="rounded-full border border-ink/10 px-3 py-1 text-sm text-ink/70 transition hover:border-ink/30 hover:text-ink"
            @click="emit('cancel')"
          >
            Close
          </button>
        </div>

        <label class="mt-6 block text-sm font-semibold text-ink">
          Description
          <textarea
            v-model="description"
            rows="5"
            class="mt-2 w-full rounded-[24px] border border-ink/10 bg-paper/40 px-4 py-3 text-base text-ink outline-none transition focus:border-copper/50 focus:ring-2 focus:ring-copper/20"
            placeholder="Describe the work clearly and briefly."
          />
        </label>

        <label class="mt-5 block text-sm font-semibold text-ink">
          Priority
          <select
            v-model="priority"
            class="mt-2 w-full rounded-full border border-ink/10 bg-paper/40 px-4 py-3 text-sm text-ink outline-none transition focus:border-copper/50 focus:ring-2 focus:ring-copper/20"
          >
            <option value="high">
              High
            </option>
            <option value="medium">
              Medium
            </option>
            <option value="low">
              Low
            </option>
          </select>
        </label>

        <div class="mt-6 flex justify-end gap-3">
          <button
            type="button"
            class="rounded-full border border-ink/15 px-4 py-2 text-sm font-medium text-ink transition hover:border-ink/30"
            @click="emit('cancel')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-full bg-copper px-5 py-2 text-sm font-semibold text-white transition hover:bg-copper/90 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="busy || description.trim().length === 0"
            @click="submit"
          >
            {{ busy ? 'Saving...' : 'Save changes' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
