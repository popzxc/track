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
  <section class="rounded-[36px] border border-white/80 bg-white/78 p-6 shadow-panel">
    <div class="flex flex-col gap-8 lg:flex-row lg:items-end lg:justify-between">
      <div class="max-w-2xl">
        <p class="text-xs font-semibold uppercase tracking-[0.26em] text-copper/80">
          Local issue desk
        </p>
        <h1 class="mt-3 font-display text-5xl leading-none text-ink sm:text-6xl">
          Track work without the ceremony.
        </h1>
        <p class="mt-4 max-w-xl text-sm leading-7 text-ink/65 sm:text-base">
          Your task files stay readable on disk, while the UI keeps project context, priority, and quick actions one click away.
        </p>
      </div>

      <div class="rounded-[28px] border border-ink/8 bg-paper/55 px-5 py-4 text-right">
        <p class="text-xs uppercase tracking-[0.22em] text-ink/45">
          Visible tasks
        </p>
        <p class="mt-2 font-display text-4xl text-ink">
          {{ taskCount }}
        </p>
      </div>
    </div>

    <div class="mt-8 flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
      <div class="grid gap-4 md:grid-cols-[minmax(0,220px)_auto]">
        <label class="text-sm font-semibold text-ink">
          Project
          <select
            class="mt-2 w-full rounded-full border border-ink/10 bg-white/80 px-4 py-3 text-sm text-ink outline-none transition focus:border-copper/50 focus:ring-2 focus:ring-copper/20"
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

        <label class="flex items-end gap-3 rounded-full border border-ink/10 bg-white/80 px-4 py-3 text-sm font-medium text-ink">
          <input
            type="checkbox"
            class="h-4 w-4 rounded border-ink/30 text-copper focus:ring-copper/30"
            :checked="showClosed"
            @change="emit('update:showClosed', ($event.target as HTMLInputElement).checked)"
          />
          <span>Show closed tasks</span>
        </label>
      </div>

      <button
        type="button"
        class="rounded-full bg-ink px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink/92 disabled:cursor-not-allowed disabled:opacity-60"
        :disabled="busy"
        @click="emit('refresh')"
      >
        {{ busy ? 'Refreshing...' : 'Refresh' }}
      </button>
    </div>
  </section>
</template>
