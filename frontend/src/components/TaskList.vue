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
      class="border border-red/30 bg-red/10 px-4 py-3 text-sm text-red shadow-panel"
    >
      {{ errorMessage }}
    </div>

    <div
      v-if="loading"
      class="border border-fg2/20 bg-bg1/95 px-5 py-10 text-center text-sm text-fg3 shadow-panel"
    >
      Loading tasks...
    </div>

    <div
      v-else-if="tasks.length === 0"
      class="border border-dashed border-fg2/20 bg-bg1/95 px-5 py-12 text-center shadow-panel"
    >
      <p class="font-display text-2xl text-fg0 sm:text-3xl">
        Queue is empty.
      </p>
      <p class="mt-3 text-sm leading-6 text-fg2">
        New tasks created from the CLI will appear here as soon as they are written to disk.
      </p>
    </div>

    <div v-else class="space-y-3">
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
