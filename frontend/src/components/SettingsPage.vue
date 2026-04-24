<script setup lang="ts">
import type {
  RemoteAgentSettings,
  RemoteCleanupSummary,
  RemoteResetSummary,
} from '../types/task'

const props = defineProps<{
  activeRemoteWorkCount: number
  cleaningUpRemoteArtifacts: boolean
  cleanupSummary: RemoteCleanupSummary | null
  remoteAgentSettings: RemoteAgentSettings | null
  resettingRemoteWorkspace: boolean
  resetSummary: RemoteResetSummary | null
  runnerSetupReady: boolean
  shellPreludeHelpText: string
}>()

const emit = defineEmits<{
  'request-open-cleanup': []
  'request-open-reset': []
  'request-open-runner-setup': []
}>()

function remoteAgentToolLabel(tool: RemoteAgentSettings['preferredTool'] | undefined): string {
  switch (tool) {
    case 'claude': return 'Claude'
    case 'codex':
    default: return 'Codex'
  }
}
</script>

<template>
  <section class="space-y-4">
    <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
        Settings
      </h1>
      <p class="mt-2 text-sm text-fg3">
        Remote runner configuration for task dispatches and PR reviews
      </p>
    </div>

    <section class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
        <span
          class="border px-2 py-1"
          :class="
            runnerSetupReady
              ? 'border-aqua/30 bg-aqua/10 text-aqua'
              : remoteAgentSettings?.configured
                ? 'border-yellow/30 bg-yellow/10 text-yellow'
                : 'border-fg2/20 bg-bg0 text-fg2'
          "
        >
          {{
            runnerSetupReady
              ? 'Runner ready'
              : remoteAgentSettings?.configured
                ? 'Runner needs shell prelude'
                : 'Remote dispatch not configured'
          }}
        </span>
      </div>

      <div class="mt-5 grid gap-4 md:grid-cols-2 xl:grid-cols-4 2xl:grid-cols-8">
        <dl class="contents">
          <div class="border border-fg2/15 bg-bg0/60 p-4">
            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
              Host
            </dt>
            <dd class="mt-2 break-all text-sm text-fg1">
              {{ remoteAgentSettings?.host || 'Not configured' }}
            </dd>
          </div>
          <div class="border border-fg2/15 bg-bg0/60 p-4">
            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
              User
            </dt>
            <dd class="mt-2 break-all text-sm text-fg1">
              {{ remoteAgentSettings?.user || 'Not configured' }}
            </dd>
          </div>
          <div class="border border-fg2/15 bg-bg0/60 p-4">
            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
              Port
            </dt>
            <dd class="mt-2 text-sm text-fg1">
              {{ remoteAgentSettings?.port ?? 22 }}
            </dd>
          </div>
          <div class="border border-fg2/15 bg-bg0/60 p-4">
            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
              Shell prelude
            </dt>
            <dd class="mt-2 text-sm text-fg1">
              {{ runnerSetupReady ? 'Configured' : 'Missing' }}
            </dd>
          </div>
          <div class="border border-fg2/15 bg-bg0/60 p-4">
            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
              Preferred tool
            </dt>
            <dd class="mt-2 text-sm text-fg1">
              {{ remoteAgentToolLabel(remoteAgentSettings?.preferredTool) }}
            </dd>
          </div>
          <div class="border border-fg2/15 bg-bg0/60 p-4">
            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
              Automatic follow-up
            </dt>
            <dd class="mt-2 text-sm text-fg1">
              {{ remoteAgentSettings?.reviewFollowUp?.enabled ? 'Enabled' : 'Disabled' }}
            </dd>
          </div>
          <div class="border border-fg2/15 bg-bg0/60 p-4">
            <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
              Main user
            </dt>
            <dd class="mt-2 text-sm text-fg1">
              {{ remoteAgentSettings?.reviewFollowUp?.mainUser || 'Not set' }}
            </dd>
          </div>
        </dl>

        <button
          type="button"
          data-testid="edit-runner-setup-button"
          class="flex h-full items-center justify-center border border-aqua/35 bg-aqua/10 px-4 py-3 text-center text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
          @click="emit('request-open-runner-setup')"
        >
          Edit runner setup
        </button>
      </div>

      <div class="mt-6 space-y-4">
        <section class="border border-fg2/15 bg-bg0/60 p-4">
          <div class="flex items-start justify-between gap-4">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Current shell prelude
            </p>
            <span
              :title="shellPreludeHelpText"
              aria-label="Why the shell prelude exists"
              tabindex="0"
              class="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-fg2/20 bg-bg1/60 text-xs font-semibold text-fg2 transition hover:border-fg1/35 hover:text-fg0 focus:border-aqua/50 focus:text-fg0 focus:outline-none"
            >
              i
            </span>
          </div>
          <pre class="mt-4 overflow-x-auto whitespace-pre-wrap text-sm leading-7 text-fg1">{{ remoteAgentSettings?.shellPrelude || 'No shell prelude has been saved yet.' }}</pre>
        </section>

        <section class="border border-fg2/15 bg-bg0/60 p-4">
          <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Default review prompt
          </p>
          <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
            {{ remoteAgentSettings?.reviewFollowUp?.defaultReviewPrompt || 'Not set' }}
          </div>
        </section>
      </div>

      <section class="mt-4 border border-fg2/15 bg-bg0/60 p-4">
        <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div class="min-w-0">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Manual cleanup
            </p>
            <div class="mt-4 space-y-4 text-sm leading-7 text-fg1">
              <p>
                Sweep the remote workspace for stale task artifacts that are no longer needed.
              </p>
              <p>
                Open tasks keep their tracked worktrees. Closed tasks keep metadata but release worktrees. Missing tasks lose both remote artifacts and their saved local dispatch history.
              </p>
            </div>
          </div>

          <button
            type="button"
            data-testid="settings-cleanup-button"
            class="border border-orange/30 bg-orange/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-orange transition hover:bg-orange/15 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="cleaningUpRemoteArtifacts || !remoteAgentSettings?.configured"
            @click="emit('request-open-cleanup')"
          >
            {{ cleaningUpRemoteArtifacts ? 'Cleaning up...' : 'Clean up remote artifacts' }}
          </button>
        </div>

        <div
          v-if="cleanupSummary"
          data-testid="cleanup-summary"
          class="mt-4 border border-fg2/15 bg-bg1/70 p-4"
        >
          <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
            Last cleanup result
          </p>
          <dl class="mt-4 grid gap-3 text-sm md:grid-cols-2 xl:grid-cols-5">
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                Closed tasks
              </dt>
              <dd class="mt-1 text-fg1">
                {{ cleanupSummary.closedTasksCleaned }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                Missing tasks
              </dt>
              <dd class="mt-1 text-fg1">
                {{ cleanupSummary.missingTasksCleaned }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                Local histories
              </dt>
              <dd class="mt-1 text-fg1">
                {{ cleanupSummary.localDispatchHistoriesRemoved }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                Worktrees
              </dt>
              <dd class="mt-1 text-fg1">
                {{ cleanupSummary.remoteWorktreesRemoved }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                Run dirs
              </dt>
              <dd class="mt-1 text-fg1">
                {{ cleanupSummary.remoteRunDirectoriesRemoved }}
              </dd>
            </div>
          </dl>
        </div>
      </section>

      <section class="mt-4 border border-fg2/15 bg-bg0/60 p-4">
        <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div class="min-w-0">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Remote reset
            </p>
            <div class="mt-4 space-y-4 text-sm leading-7 text-fg1">
              <p>
                Remove the entire remote workspace managed by <code>track</code> and delete the remote projects registry, while keeping local tasks and local dispatch history intact.
              </p>
              <p>
                Use this when the remote VM has drifted into an ambiguous state and you want the next dispatch to rebuild everything from local tracker data.
              </p>
              <p
                v-if="activeRemoteWorkCount > 0"
                class="text-yellow"
              >
                Stop active task runs and PR reviews before resetting the remote workspace.
              </p>
            </div>
          </div>

          <button
            type="button"
            data-testid="settings-reset-button"
            class="border border-red/30 bg-red/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-red transition hover:bg-red/15 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="resettingRemoteWorkspace || !remoteAgentSettings?.configured || activeRemoteWorkCount > 0"
            @click="emit('request-open-reset')"
          >
            {{ resettingRemoteWorkspace ? 'Resetting...' : 'Reset remote workspace' }}
          </button>
        </div>

        <div
          v-if="resetSummary"
          data-testid="reset-summary"
          class="mt-4 border border-fg2/15 bg-bg1/70 p-4"
        >
          <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
            Last reset result
          </p>
          <dl class="mt-4 grid gap-3 text-sm md:grid-cols-2">
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                Workspace entries
              </dt>
              <dd class="mt-1 text-fg1">
                {{ resetSummary.workspaceEntriesRemoved }}
              </dd>
            </div>
            <div>
              <dt class="text-[11px] font-semibold uppercase tracking-[0.12em] text-fg3">
                Registry
              </dt>
              <dd class="mt-1 text-fg1">
                {{ resetSummary.registryRemoved ? 'Removed' : 'Already missing' }}
              </dd>
            </div>
          </dl>
        </div>
      </section>
    </section>
  </section>
</template>
