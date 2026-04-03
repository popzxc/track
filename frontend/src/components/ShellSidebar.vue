<script setup lang="ts">
import { computed } from 'vue'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

const props = defineProps<{
  activePage: AppPage
  activeRemoteWorkCount: number
  remoteAgentConfigured: boolean
  reviewCount: number
  runnerSetupReady: boolean
  totalProjectCount: number
  visibleTaskCount: number
}>()

const emit = defineEmits<{
  navigate: [page: AppPage]
}>()

interface NavItem {
  page: AppPage
  label: string
  value: number | string
}

// The sidebar is stable shell chrome rather than page-specific UI. Keeping it
// separate makes App.vue read like screen composition instead of layout markup.
const navItems = computed<NavItem[]>(() => [
  { page: 'tasks', label: 'Tasks', value: props.visibleTaskCount },
  { page: 'reviews', label: 'Reviews', value: props.reviewCount },
  { page: 'runs', label: 'Runs', value: props.activeRemoteWorkCount },
  { page: 'projects', label: 'Projects', value: props.totalProjectCount },
  { page: 'settings', label: 'Settings', value: 'remote' },
])
</script>

<template>
  <aside class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel lg:sticky lg:top-4 lg:self-start">
    <div class="flex items-center justify-between gap-3 border-b border-fg2/10 pb-4">
      <p class="font-display text-3xl text-fg0">
        track
      </p>

      <span
        class="border px-3 py-2 text-xs font-semibold tracking-[0.08em]"
        :class="
          runnerSetupReady
            ? 'border-aqua/30 bg-aqua/10 text-aqua'
            : remoteAgentConfigured
              ? 'border-yellow/30 bg-yellow/10 text-yellow'
              : 'border-fg2/20 bg-bg0 text-fg2'
        "
      >
        {{
          runnerSetupReady
            ? 'ready'
            : remoteAgentConfigured
              ? 'setup'
              : 'local'
        }}
      </span>
    </div>

    <nav class="mt-4 space-y-2">
      <button
        v-for="item in navItems"
        :key="item.page"
        :data-testid="`shell-nav-${item.page}`"
        type="button"
        class="flex w-full items-center justify-between border px-3 py-3 text-left text-sm tracking-[0.08em] transition"
        :class="
          activePage === item.page
            ? 'border-aqua/35 bg-aqua/10 text-aqua'
            : 'border-fg2/20 bg-bg0 text-fg1 hover:border-fg1/35 hover:text-fg0'
        "
        @click="emit('navigate', item.page)"
      >
        <span>{{ item.label }}</span>
        <span class="text-xs text-fg3">{{ item.value }}</span>
      </button>
    </nav>

    <div class="mt-6 border-t border-fg2/10 pt-4 text-sm text-fg2">
      <div class="flex items-center justify-between">
        <span>Active remote work</span>
        <span>{{ activeRemoteWorkCount }}</span>
      </div>
      <div class="mt-2 flex items-center justify-between">
        <span>Visible tasks</span>
        <span>{{ visibleTaskCount }}</span>
      </div>
      <div class="mt-2 flex items-center justify-between">
        <span>Reviews</span>
        <span>{{ reviewCount }}</span>
      </div>
      <div class="mt-2 flex items-center justify-between">
        <span>Projects</span>
        <span>{{ totalProjectCount }}</span>
      </div>
    </div>
  </aside>
</template>
