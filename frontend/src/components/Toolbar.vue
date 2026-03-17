<script setup lang="ts">
import type { ProjectInfo } from '../types/task'

const props = defineProps<{
  busy?: boolean
  projects: ProjectInfo[]
  selectedProject: string
  showClosed: boolean
  taskCount: number
}>()

const emit = defineEmits<{
  refresh: []
  'update:selectedProject': [value: string]
  'update:showClosed': [value: boolean]
}>()
</script>

<template>
  <section class="border border-fg2/20 bg-bg1/95 p-3 shadow-panel">
    <div class="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
        <p class="font-semibold text-fg0">
          track
        </p>
        <span
          class="border px-2 py-1 text-xs"
          :class="busy ? 'border-yellow/30 bg-yellow/10 text-yellow' : 'border-aqua/30 bg-aqua/10 text-aqua'"
        >
          {{ busy ? 'syncing' : 'ready' }}
        </span>
        <p class="text-fg3">
          visible <span class="text-fg1">{{ taskCount }}</span>
        </p>
        <p class="text-fg3">
          projects <span class="text-fg1">{{ props.projects.length }}</span>
        </p>
      </div>

      <div class="flex flex-col gap-3 lg:flex-row lg:flex-wrap lg:items-center lg:justify-end">
        <label class="flex items-center gap-2 text-sm text-fg2">
          <span class="text-fg3">Project</span>
          <select
            class="min-w-[220px] border border-fg2/20 bg-bg0 px-3 py-2 text-sm text-fg1 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
            :value="selectedProject"
            @change="emit('update:selectedProject', ($event.target as HTMLSelectElement).value)"
          >
            <option value="">
              All projects
            </option>
            <option v-for="project in props.projects" :key="project.canonicalName" :value="project.canonicalName">
              {{ project.canonicalName }}
            </option>
          </select>
        </label>

        <label class="flex items-center gap-2 border border-fg2/10 bg-bg0 px-3 py-2 text-sm text-fg2">
          <input
            type="checkbox"
            class="h-4 w-4 rounded-none border-fg2/40 bg-bg0 accent-yellow focus:ring-0"
            :checked="showClosed"
            @change="emit('update:showClosed', ($event.target as HTMLInputElement).checked)"
          />
          <span>Include closed</span>
        </label>

        <button
          type="button"
          class="border border-fg2/20 bg-fg0/5 px-4 py-2 text-sm text-fg0 transition hover:border-aqua/35 hover:bg-aqua/12 disabled:opacity-60"
          :disabled="busy"
          @click="emit('refresh')"
        >
          {{ busy ? 'Syncing...' : 'Refresh' }}
        </button>
      </div>
    </div>
  </section>
</template>
