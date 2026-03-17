<script setup lang="ts">
import { computed } from 'vue'

import type { Task } from '../types/task'

const props = defineProps<{
  task: Task
}>()

const emit = defineEmits<{
  close: [task: Task]
  delete: [task: Task]
  edit: [task: Task]
  reopen: [task: Task]
}>()

// The filesystem path is the task's stable identity in the backend, so we
// surface a compact file-derived reference without overwhelming the row.
const taskReference = computed(() => {
  const rawReference = props.task.id.split('/').pop() ?? props.task.id
  return rawReference.replace(/\.md$/i, '')
})

const priorityBadgeClass = computed(() => {
  switch (props.task.priority) {
    case 'high':
      return 'border-red/30 bg-red/10 text-red'
    case 'medium':
      return 'border-yellow/30 bg-yellow/10 text-yellow'
    default:
      return 'border-aqua/30 bg-aqua/10 text-aqua'
  }
})

const statusBadgeClass = computed(() =>
  props.task.status === 'open'
    ? 'border-blue/30 bg-blue/10 text-blue'
    : 'border-fg2/20 bg-bg3/60 text-fg2',
)

const frameClass = computed(() => {
  switch (props.task.priority) {
    case 'high':
      return 'border-l-red'
    case 'medium':
      return 'border-l-yellow'
    default:
      return 'border-l-aqua'
  }
})

const timestampLabel = computed(() =>
  props.task.updatedAt === props.task.createdAt ? 'Created' : 'Updated',
)

const formattedTimestamp = computed(() =>
  new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(props.task.updatedAt === props.task.createdAt ? props.task.createdAt : props.task.updatedAt)),
)
</script>

<template>
  <article
    class="border border-fg2/20 border-l-4 bg-bg1/95 p-4 shadow-panel transition-colors hover:border-fg2/35"
    :class="frameClass"
  >
    <div class="grid gap-4 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-start">
      <div class="min-w-0">
        <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-fg3">
          <span>{{ task.project }}</span>
          <span class="text-fg3/40">/</span>
          <span>{{ task.source ?? 'manual' }}</span>
          <span class="text-fg3/40">/</span>
          <span class="text-fg2">{{ taskReference }}</span>
        </div>

        <h3 class="mt-3 whitespace-pre-line text-lg leading-7 text-fg0 sm:text-xl">
          {{ task.description }}
        </h3>

        <div class="mt-4 flex flex-wrap gap-2 text-[11px] font-semibold tracking-[0.08em]">
          <span class="border px-2 py-1" :class="priorityBadgeClass">
            {{ task.priority }}
          </span>
          <span class="border px-2 py-1" :class="statusBadgeClass">
            {{ task.status }}
          </span>
        </div>

        <p class="mt-4 text-xs tracking-[0.08em] text-fg3">
          {{ timestampLabel }} {{ formattedTimestamp }}
        </p>
      </div>

      <div class="flex shrink-0 flex-wrap gap-2 xl:max-w-[260px] xl:justify-end">
        <button
          type="button"
          class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/45 hover:text-fg0"
          @click="emit('edit', task)"
        >
          Edit
        </button>
        <button
          v-if="task.status === 'open'"
          type="button"
          class="border border-green/30 bg-green/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-green transition hover:bg-green/15"
          @click="emit('close', task)"
        >
          Close
        </button>
        <button
          v-else
          type="button"
          class="border border-yellow/30 bg-yellow/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-yellow transition hover:bg-yellow/15"
          @click="emit('reopen', task)"
        >
          Reopen
        </button>
        <button
          type="button"
          class="border border-red/30 bg-red/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-red transition hover:bg-red/15"
          @click="emit('delete', task)"
        >
          Delete
        </button>
      </div>
    </div>
  </article>
</template>
