<script setup lang="ts">
import type { MigrationImportSummary, MigrationStatus } from '../types/task'

const props = defineProps<{
  migrationImportPending: boolean
  migrationImportSummary: MigrationImportSummary | null
  migrationRequired: boolean
  migrationStatus: MigrationStatus | null
}>()

const emit = defineEmits<{
  'request-import-legacy-data': []
}>()

function migrationCleanupCommand(path: string) {
  return path.endsWith('.json') ? `rm -f ${path}` : `rm -rf ${path}`
}
</script>

<template>
  <section v-if="migrationImportSummary" class="space-y-4">
    <div class="border border-green/25 bg-green/8 p-4 text-sm leading-7 text-green shadow-panel">
      Imported {{ migrationImportSummary.importedTasks }} tasks, {{ migrationImportSummary.importedProjects }} projects, and {{ migrationImportSummary.importedReviews }} reviews into the SQLite backend.
    </div>

    <div
      v-if="migrationImportSummary.cleanupCandidates.length > 0"
      class="border border-fg2/15 bg-bg1/95 p-4 shadow-panel"
    >
      <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
        Optional legacy cleanup
      </p>
      <p class="mt-3 text-sm leading-7 text-fg2">
        After you confirm the imported data looks correct, run these commands on the host. Start with <code class="font-mono text-fg1">track configure</code> so the CLI materializes <code class="font-mono text-fg1">~/.config/track/cli.json</code>, then reinstall <code class="font-mono text-fg1">cargo-airbender</code> from your <code class="font-mono text-fg1">airbender-platform</code> checkout.
      </p>
      <div class="mt-4 overflow-x-auto border border-fg2/10 bg-bg0/60 px-4 py-4 font-mono text-xs leading-7 text-fg1">
        <p>track configure</p>
        <p>cargo install --path crates/cargo-airbender --force</p>
        <p
          v-for="candidate in migrationImportSummary.cleanupCandidates"
          :key="candidate.path"
        >
          {{ migrationCleanupCommand(candidate.path) }}
        </p>
      </div>
      <p class="mt-3 text-sm leading-7 text-fg3">
        Keep <code class="font-mono text-fg1">~/.track/models</code> if you use local capture.
      </p>
    </div>
  </section>

  <section v-if="migrationRequired && migrationStatus" class="space-y-4">
    <div class="border border-yellow/25 bg-yellow/8 p-5 shadow-panel">
      <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-yellow">
        Migration required
      </p>
      <h1 class="mt-3 font-display text-3xl text-fg0 sm:text-4xl">
        Import legacy track data before using the app
      </h1>
      <p class="mt-4 max-w-3xl text-sm leading-7 text-fg2">
        This backend uses SQLite-backed state. Legacy Markdown and JSON data were detected, so normal API routes stay gated until that data is imported.
      </p>

      <div class="mt-6 grid gap-4 md:grid-cols-3">
        <div class="border border-fg2/15 bg-bg0/60 p-4">
          <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">Projects</p>
          <p class="mt-2 text-2xl text-fg0">{{ migrationStatus.summary.projectsFound }}</p>
        </div>
        <div class="border border-fg2/15 bg-bg0/60 p-4">
          <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">Tasks</p>
          <p class="mt-2 text-2xl text-fg0">{{ migrationStatus.summary.tasksFound }}</p>
        </div>
        <div class="border border-fg2/15 bg-bg0/60 p-4">
          <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">Reviews</p>
          <p class="mt-2 text-2xl text-fg0">{{ migrationStatus.summary.reviewsFound }}</p>
        </div>
      </div>

      <div class="mt-6 flex flex-wrap gap-3">
        <button
          type="button"
          data-testid="migration-import-button"
          class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:cursor-not-allowed disabled:opacity-60"
          :disabled="migrationImportPending || !migrationStatus.canImport"
          @click="emit('request-import-legacy-data')"
        >
          {{ migrationImportPending ? 'Importing...' : 'Import legacy data' }}
        </button>
      </div>
    </div>

    <div
      v-if="migrationStatus.skippedRecords.length > 0"
      class="border border-fg2/15 bg-bg1/95 p-4 shadow-panel"
    >
      <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
        Skipped legacy records
      </p>
      <ul class="mt-4 space-y-3 text-sm leading-6 text-fg2">
        <li
          v-for="record in migrationStatus.skippedRecords.slice(0, 5)"
          :key="`${record.kind}:${record.path}`"
          class="border border-fg2/10 bg-bg0/50 px-3 py-3"
        >
          <p class="font-semibold text-fg1">{{ record.kind }}</p>
          <p class="mt-1 break-all">{{ record.path }}</p>
          <p class="mt-2 text-fg3">{{ record.error }}</p>
        </li>
      </ul>
    </div>
  </section>
</template>
