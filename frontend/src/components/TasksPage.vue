<script setup lang="ts">
import { computed } from 'vue'

import {
  dispatchBadgeClass,
  dispatchStatusLabel,
  formatTaskTimestamp,
  priorityBadgeClass,
  taskReference,
  taskStatusBadgeClass,
  type TaskGroup,
} from '../features/tasks/presentation'
import { taskTitle } from '../features/tasks/description'
import type { ProjectInfo, TaskDispatch } from '../types/task'

const props = defineProps<{
  activeTaskId: string | null
  drawerOpen: boolean
  latestDispatchByTaskId: Record<string, TaskDispatch>
  projects: ProjectInfo[]
  selectedProjectFilter: string
  showClosed: boolean
  taskCount: number
  taskGroups: TaskGroup[]
}>()

const emit = defineEmits<{
  'request-create-task': []
  'request-select-task': [taskId: string]
  'update:selectedProjectFilter': [value: string]
  'update:showClosed': [value: boolean]
}>()

// App.vue still owns the actual task query and selection state. This component
// only renders the queue controls and rows so later refactors can move state
// behind a smaller, already-tested surface.
const selectedProjectFilterModel = computed({
  get: () => props.selectedProjectFilter,
  set: (value: string) => emit('update:selectedProjectFilter', value),
})

const showClosedModel = computed({
  get: () => props.showClosed,
  set: (value: boolean) => emit('update:showClosed', value),
})
</script>

<template>
  <section class="space-y-4">
    <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <div class="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
        <div>
          <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
            Tasks
          </h1>
        </div>

        <div class="flex flex-wrap items-center gap-3">
          <select
            v-model="selectedProjectFilterModel"
            data-testid="task-project-filter"
            class="min-w-[220px] border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
          >
            <option value="">
              All projects
            </option>
            <option
              v-for="project in projects"
              :key="project.canonicalName"
              :value="project.canonicalName"
            >
              {{ project.canonicalName }}
            </option>
          </select>

          <label class="flex items-center gap-3 border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg1">
            <input
              v-model="showClosedModel"
              data-testid="task-show-closed"
              type="checkbox"
              class="h-4 w-4 border-fg2/30 bg-bg0 text-aqua focus:ring-aqua/50"
            />
            Closed
          </label>

          <button
            type="button"
            data-testid="new-task-button"
            class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
            @click="emit('request-create-task')"
          >
            New task
          </button>
        </div>
      </div>
    </div>

    <div v-if="taskCount === 0" class="border border-fg2/20 bg-bg1/95 px-4 py-12 text-center shadow-panel">
      <p class="font-display text-2xl text-fg0">
        Queue is empty.
      </p>
      <p class="mt-3 text-sm leading-6 text-fg2">
        New tasks from the CLI or the web form will appear here.
      </p>
    </div>

    <div v-else class="space-y-4">
      <section
        v-for="group in taskGroups"
        :key="group.project"
        :data-project="group.project"
        data-testid="task-group"
        class="overflow-hidden border border-fg2/20 bg-bg1/95 shadow-panel"
      >
        <div class="border-b border-fg2/10 bg-bg0/35 px-4 py-3">
          <div class="flex items-center justify-between gap-3">
            <p class="text-[11px] font-semibold uppercase tracking-[0.22em] text-fg2">
              {{ group.project }}
            </p>
            <span class="text-xs text-fg3">{{ group.tasks.length }}</span>
          </div>
        </div>

        <div class="divide-y divide-fg2/10">
          <button
            v-for="task in group.tasks"
            :key="task.id"
            type="button"
            :data-task-id="task.id"
            data-testid="task-row"
            class="w-full px-4 py-5 text-left transition hover:bg-bg0/40"
            :class="activeTaskId === task.id && drawerOpen ? 'bg-bg0/55' : 'bg-transparent'"
            @click="emit('request-select-task', task.id)"
          >
            <div class="space-y-3">
              <p class="text-xs tracking-[0.08em] text-fg3">
                {{ task.source ?? 'manual' }} / {{ taskReference(task) }}
              </p>

              <p class="whitespace-pre-wrap text-xl leading-8 text-fg0">
                {{ taskTitle(task) }}
              </p>

              <div class="flex flex-wrap items-center gap-2">
                <span
                  class="border px-2 py-1 text-[11px] font-semibold tracking-[0.08em]"
                  :class="priorityBadgeClass(task.priority)"
                >
                  {{ task.priority }}
                </span>
                <span
                  class="border px-2 py-1 text-[11px] font-semibold tracking-[0.08em]"
                  :class="taskStatusBadgeClass(task.status)"
                >
                  {{ task.status }}
                </span>
                <span
                  class="border px-2 py-1 text-[11px] font-semibold tracking-[0.08em]"
                  :class="dispatchBadgeClass(latestDispatchByTaskId[task.id])"
                >
                  {{ dispatchStatusLabel(latestDispatchByTaskId[task.id]) }}
                </span>
                <span class="text-xs tracking-[0.08em] text-fg3">
                  {{ formatTaskTimestamp(task) }}
                </span>
              </div>
            </div>
          </button>
        </div>
      </section>
    </div>
  </section>
</template>
