<script setup lang="ts">
import { ref, watch } from 'vue'

import type { Priority, ProjectInfo, Task } from '../types/task'

const props = defineProps<{
  busy?: boolean
  defaultProject?: string
  mode: 'create' | 'edit'
  open: boolean
  projects: ProjectInfo[]
  task: Task | null
}>()

const emit = defineEmits<{
  cancel: []
  save: [payload: { description: string; priority: Priority; project: string }]
}>()

const description = ref('')
const priority = ref<Priority>('medium')
const project = ref('')

watch(
  () => [props.task, props.mode, props.defaultProject, props.projects] as const,
  ([task, mode, defaultProject, projects]) => {
    description.value = task?.description ?? ''
    priority.value = task?.priority ?? 'medium'
    project.value =
      task?.project ??
      defaultProject ??
      projects[0]?.canonicalName ??
      ''

    if (mode === 'edit' && task) {
      project.value = task.project
    }
  },
  { immediate: true },
)

function submit() {
  emit('save', {
    description: description.value.trim(),
    priority: priority.value,
    project: project.value,
  })
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-bg0/80 px-4 py-6"
    >
      <div class="w-full max-w-2xl border border-fg2/20 bg-bg1 p-6 shadow-panel">
        <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-4">
          <div>
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              {{ mode === 'create' ? 'New task' : 'Edit task' }}
            </p>
            <h3 class="mt-2 font-display text-2xl text-fg0 sm:text-3xl">
              {{ mode === 'create' ? 'Create task' : 'Refine description' }}
            </h3>
            <p v-if="task" class="mt-3 text-xs tracking-[0.08em] text-fg3">
              {{ task.project }} / {{ task.source ?? 'manual' }}
            </p>
            <p v-else class="mt-3 text-xs tracking-[0.08em] text-fg3">
              Create a new task directly from the web UI.
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

        <label
          v-if="mode === 'create'"
          class="mt-6 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3"
        >
          Project
          <select
            v-model="project"
            class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
          >
            <option v-for="entry in projects" :key="entry.canonicalName" :value="entry.canonicalName">
              {{ entry.canonicalName }}
            </option>
          </select>
        </label>

        <label class="mt-6 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
          Description
          <textarea
            v-model="description"
            rows="6"
            class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
            placeholder="Describe the work clearly and briefly."
          />
        </label>

        <label class="mt-5 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
          Priority
          <select
            v-model="priority"
            class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
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
            class="border border-fg2/20 bg-bg0 px-4 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/45 hover:text-fg0"
            @click="emit('cancel')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="border border-aqua/35 bg-aqua/10 px-5 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:opacity-60"
            :disabled="busy || description.trim().length === 0 || (mode === 'create' && project.trim().length === 0)"
            @click="submit"
          >
            {{ busy ? 'Saving...' : mode === 'create' ? 'Create task' : 'Save changes' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
