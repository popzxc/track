<script setup lang="ts">
import { RouterView } from 'vue-router'

import ShellSidebar from './components/ShellSidebar.vue'
import { provideTrackerShell } from './composables/useTrackerShell'

// =============================================================================
// Routed App Shell
// =============================================================================
//
// The shell owns only global concerns now: shared data loading, background
// refresh, error presentation, and the persistent navigation chrome. Route
// components own page-specific state so browser URLs become the source of
// truth for navigation instead of an app-local string ref.
const shell = provideTrackerShell()
</script>

<template>
  <main class="min-h-screen px-4 py-4 sm:px-6 sm:py-6 lg:px-8">
    <div class="mx-auto max-w-[1800px]">
      <div class="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
        <ShellSidebar
          :active-remote-work-count="shell.activeRemoteWorkCount.value"
          :remote-agent-configured="Boolean(shell.remoteAgentSettings.value?.configured)"
          :review-count="shell.reviewCount.value"
          :runner-setup-ready="shell.runnerSetupReady.value"
          :total-project-count="shell.totalProjectCount.value"
          :visible-task-count="shell.visibleTaskCount.value"
        />

        <section class="min-w-0 space-y-4">
          <div
            v-if="shell.errorMessage.value"
            data-testid="error-banner"
            class="border border-red/30 bg-red/10 px-4 py-3 text-sm text-red shadow-panel"
          >
            {{ shell.errorMessage.value }}
          </div>

          <div
            v-if="shell.loading.value"
            class="border border-fg2/20 bg-bg1/95 px-5 py-16 text-center text-sm text-fg3 shadow-panel"
          >
            Loading tracker data...
          </div>

          <RouterView v-else />
        </section>
      </div>
    </div>
  </main>
</template>
