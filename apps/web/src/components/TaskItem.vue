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

const priorityBadgeClass = computed(() => {
  switch (props.task.priority) {
    case 'high':
      return 'bg-berry/10 text-berry ring-1 ring-berry/20'
    case 'medium':
      return 'bg-copper/10 text-copper ring-1 ring-copper/20'
    default:
      return 'bg-sage/10 text-sage ring-1 ring-sage/20'
  }
})

const statusBadgeClass = computed(() =>
  props.task.status === 'open'
    ? 'bg-ink/5 text-ink ring-1 ring-ink/10'
    : 'bg-mist text-sage ring-1 ring-sage/15',
)

const frameClass = computed(() => {
  switch (props.task.priority) {
    case 'high':
      return 'border-l-berry'
    case 'medium':
      return 'border-l-copper'
    default:
      return 'border-l-sage'
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
    class="rounded-[28px] border border-white/80 border-l-4 bg-white/90 p-5 shadow-panel transition hover:-translate-y-0.5 hover:shadow-[0_26px_48px_rgba(31,43,45,0.16)]"
    :class="frameClass"
  >
    <div class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
      <div class="space-y-3">
        <div class="flex flex-wrap items-center gap-2 text-xs font-semibold uppercase tracking-[0.18em] text-ink/45">
          <span>{{ task.project }}</span>
          <span class="h-1 w-1 rounded-full bg-ink/20" />
          <span>{{ task.source ?? 'manual' }}</span>
        </div>

        <h3 class="font-display text-2xl leading-tight text-ink">
          {{ task.description }}
        </h3>

        <div class="flex flex-wrap gap-2 text-sm">
          <span class="rounded-full px-3 py-1 font-semibold" :class="priorityBadgeClass">
            {{ task.priority }}
          </span>
          <span class="rounded-full px-3 py-1 font-semibold" :class="statusBadgeClass">
            {{ task.status }}
          </span>
        </div>

        <p class="text-sm text-ink/55">
          {{ timestampLabel }} {{ formattedTimestamp }}
        </p>
      </div>

      <div class="flex shrink-0 flex-wrap gap-2 lg:justify-end">
        <button
          type="button"
          class="rounded-full border border-ink/10 px-4 py-2 text-sm font-medium text-ink transition hover:border-ink/30"
          @click="emit('edit', task)"
        >
          Edit
        </button>
        <button
          v-if="task.status === 'open'"
          type="button"
          class="rounded-full border border-sage/20 bg-sage/8 px-4 py-2 text-sm font-medium text-sage transition hover:bg-sage/14"
          @click="emit('close', task)"
        >
          Close
        </button>
        <button
          v-else
          type="button"
          class="rounded-full border border-copper/20 bg-copper/8 px-4 py-2 text-sm font-medium text-copper transition hover:bg-copper/14"
          @click="emit('reopen', task)"
        >
          Reopen
        </button>
        <button
          type="button"
          class="rounded-full border border-berry/20 bg-berry/8 px-4 py-2 text-sm font-medium text-berry transition hover:bg-berry/14"
          @click="emit('delete', task)"
        >
          Delete
        </button>
      </div>
    </div>
  </article>
</template>
