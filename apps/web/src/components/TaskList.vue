<script setup lang="ts">
import type { Task } from '../types/task'

import TaskItem from './TaskItem.vue'

defineProps<{
  errorMessage: string
  loading: boolean
  tasks: Task[]
}>()

const emit = defineEmits<{
  close: [task: Task]
  delete: [task: Task]
  edit: [task: Task]
  reopen: [task: Task]
}>()
</script>

<template>
  <section class="space-y-4">
    <div
      v-if="errorMessage"
      class="rounded-[24px] border border-berry/20 bg-berry/8 px-5 py-4 text-sm text-berry"
    >
      {{ errorMessage }}
    </div>

    <div
      v-if="loading"
      class="rounded-[28px] border border-white/80 bg-white/70 px-5 py-8 text-center text-sm text-ink/55 shadow-panel"
    >
      Loading tasks...
    </div>

    <div
      v-else-if="tasks.length === 0"
      class="rounded-[28px] border border-dashed border-ink/15 bg-white/65 px-5 py-12 text-center shadow-panel"
    >
      <p class="font-display text-3xl text-ink">
        Nothing to wrangle right now.
      </p>
      <p class="mt-3 text-sm leading-6 text-ink/55">
        New tasks created from the CLI will appear here as soon as they are written to disk.
      </p>
    </div>

    <div v-else class="space-y-4">
      <TaskItem
        v-for="task in tasks"
        :key="task.id"
        :task="task"
        @close="emit('close', $event)"
        @delete="emit('delete', $event)"
        @edit="emit('edit', $event)"
        @reopen="emit('reopen', $event)"
      />
    </div>
  </section>
</template>
