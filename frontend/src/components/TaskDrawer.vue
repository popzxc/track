<script setup lang="ts">
import { computed } from 'vue'

import {
  dispatchBadgeClass,
  dispatchStatusLabel,
  dispatchSummary,
  formatDateTime,
  formatTaskTimestamp,
  priorityBadgeClass,
  taskReference,
  taskStatusBadgeClass,
} from '../features/tasks/presentation'
import { parseTaskDescription, taskTitle } from '../features/tasks/description'
import type { ProjectInfo, RemoteAgentPreferredTool, RunRecord, Task, TaskDispatch } from '../types/task'

type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

const props = defineProps<{
  canContinue: boolean
  canDiscardHistory: boolean
  canStartFresh: boolean
  dispatchDisabledReason?: string
  isDispatching: boolean
  isDiscardingHistory: boolean
  latestDispatch: TaskDispatch | null
  latestReusablePullRequest: string | null
  lifecycleMutation: TaskLifecycleMutation | null
  lifecycleProgressMessage: string
  pinnedTool: RemoteAgentPreferredTool | null
  primaryActionClass: string
  primaryActionDisabled: boolean
  primaryActionLabel: string
  startTool: RemoteAgentPreferredTool
  task: Task
  taskProject: ProjectInfo | null
  taskRuns: RunRecord[]
}>()

const emit = defineEmits<{
  close: []
  'request-close-task': []
  'request-delete-task': []
  'request-discard-history': []
  'request-edit-task': []
  'request-open-project': []
  'request-open-url': [url: string]
  'request-primary-action': []
  'request-start-fresh': []
  'update:startTool': [tool: RemoteAgentPreferredTool]
}>()

// This first extraction keeps App.vue responsible for selection, polling, and
// mutations. The drawer becomes a presentational boundary so later state
// refactors can happen behind a smaller template surface.
const parsedTaskDescription = computed(() => parseTaskDescription(props.task.description))

const startToolModel = computed({
  get: () => props.startTool,
  set: (tool: RemoteAgentPreferredTool) => emit('update:startTool', tool),
})

function remoteAgentToolLabel(tool: RemoteAgentPreferredTool | null | undefined): string {
  switch (tool) {
    case 'claude': return 'Claude'
    case 'opencode': return 'opencode'
    case 'codex':
    default: return 'Codex'
  }
}
</script>

<template>
  <div
    class="fixed inset-0 z-40 flex justify-end bg-bg0/70 backdrop-blur-[2px]"
    @click.self="emit('close')"
  >
    <aside
      data-testid="task-drawer"
      class="h-full w-full max-w-[1150px] overflow-y-auto border-l border-fg2/20 bg-bg1 shadow-panel"
    >
      <div class="space-y-5 p-5 sm:p-6">
        <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-5">
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-fg3">
              <button
                v-if="taskProject"
                type="button"
                class="transition hover:text-fg0"
                @click="emit('request-open-project')"
              >
                {{ task.project }}
              </button>
              <span v-else>{{ task.project }}</span>
              <span class="text-fg3/40">/</span>
              <span>{{ taskReference(task) }}</span>
            </div>

            <h2 class="mt-3 whitespace-pre-wrap font-display text-3xl leading-tight text-fg0 sm:text-4xl">
              {{ parsedTaskDescription?.title ?? taskTitle(task) }}
            </h2>

            <div class="mt-4 flex flex-wrap gap-2 text-[11px] font-semibold tracking-[0.08em]">
              <span class="border px-2 py-1" :class="priorityBadgeClass(task.priority)">
                {{ task.priority }}
              </span>
              <span class="border px-2 py-1" :class="taskStatusBadgeClass(task.status)">
                {{ task.status }}
              </span>
              <span class="border px-2 py-1" :class="dispatchBadgeClass(latestDispatch)">
                {{ dispatchStatusLabel(latestDispatch) }}
              </span>
            </div>

            <p class="mt-4 text-sm leading-7 text-fg2">
              {{ formatTaskTimestamp(task) }}
            </p>
          </div>

          <button
            type="button"
            class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="lifecycleMutation !== null"
            @click="emit('close')"
          >
            Close
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <button
            type="button"
            data-testid="drawer-primary-action"
            class="px-4 py-2.5 text-sm font-semibold tracking-[0.08em] transition disabled:cursor-not-allowed disabled:opacity-60"
            :class="primaryActionClass"
            :disabled="primaryActionDisabled"
            @click="emit('request-primary-action')"
          >
            {{ primaryActionLabel }}
          </button>

          <button
            type="button"
            class="border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg1 transition hover:border-fg1/35 hover:text-fg0 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="lifecycleMutation !== null"
            @click="emit('request-edit-task')"
          >
            Edit
          </button>

          <button
            v-if="task.status === 'open'"
            type="button"
            class="border border-green/30 bg-green/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-green transition hover:bg-green/15 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="lifecycleMutation !== null"
            @click="emit('request-close-task')"
          >
            {{ lifecycleMutation === 'closing' ? 'Closing...' : 'Close task' }}
          </button>

          <button
            v-if="latestReusablePullRequest"
            type="button"
            class="border border-aqua/30 bg-aqua/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="lifecycleMutation !== null"
            @click="emit('request-open-url', latestReusablePullRequest)"
          >
            View PR
          </button>

          <details class="relative" :class="lifecycleMutation !== null ? 'pointer-events-none opacity-60' : ''">
            <summary class="cursor-pointer list-none border border-fg2/20 bg-bg0 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0">
              More
            </summary>

            <div class="absolute right-0 z-10 mt-2 min-w-[210px] space-y-2 border border-fg2/20 bg-bg1 p-2 shadow-panel">
              <button
                v-if="canStartFresh"
                type="button"
                class="w-full border border-blue/25 bg-blue/8 px-3 py-2 text-left text-xs font-semibold tracking-[0.08em] text-blue transition hover:bg-blue/12 disabled:opacity-60"
                :disabled="isDispatching"
                @click="emit('request-start-fresh')"
              >
                {{ isDispatching ? 'Starting...' : `Start fresh via ${remoteAgentToolLabel(startTool)}` }}
              </button>

              <button
                v-if="canDiscardHistory"
                type="button"
                class="w-full border border-fg2/20 bg-bg0 px-3 py-2 text-left text-xs font-semibold tracking-[0.08em] text-fg2 transition hover:border-fg1/35 hover:text-fg0 disabled:opacity-60"
                :disabled="isDiscardingHistory"
                @click="emit('request-discard-history')"
              >
                {{ isDiscardingHistory ? 'Discarding...' : 'Discard history' }}
              </button>

              <button
                type="button"
                class="w-full border border-red/30 bg-red/10 px-3 py-2 text-left text-xs font-semibold tracking-[0.08em] text-red transition hover:bg-red/15"
                @click="emit('request-delete-task')"
              >
                {{ lifecycleMutation === 'deleting' ? 'Deleting...' : 'Delete' }}
              </button>
            </div>
          </details>
        </div>

        <section
          v-if="task.status === 'open'"
          class="border border-fg2/15 bg-bg0/60 p-4"
        >
          <div
            v-if="!pinnedTool"
            class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between"
          >
            <label class="block min-w-[220px] text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Dispatch via
              <select
                v-model="startToolModel"
                data-testid="drawer-dispatch-tool"
                class="mt-2 w-full border border-fg2/20 bg-bg1 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
              >
                <option value="codex">
                  Codex
                </option>
                <option value="claude">
                  Claude
                </option>
                <option value="opencode">
                  opencode
                </option>
              </select>
            </label>
          </div>

          <p
            v-else
            data-testid="drawer-pinned-tool"
            class="max-w-2xl text-sm leading-7 text-fg2"
          >
            This task stays on
            <span class="text-fg0">{{ remoteAgentToolLabel(pinnedTool) }}</span>
            for future dispatches.
          </p>
        </section>

        <p
          v-if="lifecycleMutation"
          class="border border-blue/20 bg-blue/8 px-4 py-3 text-sm leading-6 text-blue"
        >
          {{ lifecycleProgressMessage }}
        </p>

        <p
          v-if="dispatchDisabledReason && task.status === 'open' && !canContinue"
          class="border border-yellow/25 bg-yellow/8 px-4 py-3 text-sm leading-6 text-yellow"
        >
          {{ dispatchDisabledReason }}
        </p>

        <section class="border border-fg2/15 bg-bg0/60 p-4">
          <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Summary
          </p>
          <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
            {{ parsedTaskDescription?.summaryMarkdown || task.description }}
          </div>
        </section>

        <section v-if="parsedTaskDescription?.originalNote" class="space-y-3">
          <details class="border border-fg2/15 bg-bg0/60 p-4">
            <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Original note
            </summary>
            <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
              {{ parsedTaskDescription.originalNote }}
            </div>
          </details>
        </section>

        <section class="border border-fg2/15 bg-bg0/60 p-4">
          <div class="flex items-center justify-between gap-3">
            <div>
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Run history
              </p>
              <p class="mt-2 text-sm text-fg2">
                Every dispatch attempt is kept here so you can continue or start fresh with context.
              </p>
            </div>
            <span class="text-xs text-fg3">{{ taskRuns.length }}</span>
          </div>

          <div
            v-if="taskRuns.length === 0"
            class="mt-4 border border-dashed border-fg2/15 px-4 py-6 text-sm text-fg3"
          >
            This task has no run history yet.
          </div>

          <div v-else class="mt-4 space-y-3">
            <article
              v-for="(run, index) in taskRuns"
              :key="run.dispatch.dispatchId"
              :data-dispatch-id="run.dispatch.dispatchId"
              data-testid="run-history-item"
              class="border border-fg2/15 bg-bg1/70 p-4"
            >
              <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <div class="flex flex-wrap items-center gap-2 text-[11px] font-semibold tracking-[0.08em]">
                    <span
                      v-if="index === 0"
                      data-testid="run-latest-badge"
                      class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2"
                    >
                      Latest
                    </span>
                    <span class="border px-2 py-1" :class="dispatchBadgeClass(run.dispatch)">
                      {{ dispatchStatusLabel(run.dispatch) }}
                    </span>
                    <span class="border border-fg2/15 bg-bg0 px-2 py-1 text-fg2">
                      via {{ remoteAgentToolLabel(run.dispatch.preferredTool) }}
                    </span>
                    <span class="text-fg3">Started {{ formatDateTime(run.dispatch.createdAt) }}</span>
                    <span v-if="run.dispatch.followUpRequest" class="text-fg3">• Follow-up</span>
                  </div>
                </div>

                <button
                  v-if="run.dispatch.pullRequestUrl"
                  type="button"
                  class="border border-aqua/30 bg-aqua/10 px-3 py-2 text-xs font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                  @click="emit('request-open-url', run.dispatch.pullRequestUrl)"
                >
                  View PR
                </button>
              </div>

              <p class="mt-4 text-sm leading-7 text-fg1">
                {{ dispatchSummary(run.dispatch) }}
              </p>

              <dl class="mt-4 grid gap-4 text-sm md:grid-cols-2">
                <div>
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Started
                  </dt>
                  <dd class="mt-1 text-fg1">
                    {{ formatDateTime(run.dispatch.createdAt) }}
                  </dd>
                </div>
                <div v-if="run.dispatch.finishedAt">
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Finished
                  </dt>
                  <dd class="mt-1 text-fg1">
                    {{ formatDateTime(run.dispatch.finishedAt) }}
                  </dd>
                </div>
                <div v-if="run.dispatch.branchName">
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Branch
                  </dt>
                  <dd class="mt-1 break-all text-fg1">
                    {{ run.dispatch.branchName }}
                  </dd>
                </div>
                <div v-if="run.dispatch.worktreePath">
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Worktree
                  </dt>
                  <dd class="mt-1 break-all text-fg1">
                    {{ run.dispatch.worktreePath }}
                  </dd>
                </div>
              </dl>

              <details
                v-if="run.dispatch.followUpRequest"
                class="mt-4 border border-aqua/20 bg-aqua/6 p-4"
              >
                <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-aqua">
                  Follow-up request
                </summary>
                <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                  {{ run.dispatch.followUpRequest }}
                </div>
              </details>

              <details
                v-if="run.dispatch.notes"
                class="mt-4 border border-fg2/15 bg-bg0/70 p-4"
              >
                <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Run notes
                </summary>
                <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                  {{ run.dispatch.notes }}
                </div>
              </details>

              <details
                v-if="run.dispatch.errorMessage"
                class="mt-4 border border-red/20 bg-red/5 p-4"
              >
                <summary class="cursor-pointer text-[11px] font-semibold uppercase tracking-[0.16em] text-red">
                  Error details
                </summary>
                <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-red">
                  {{ run.dispatch.errorMessage }}
                </div>
              </details>
            </article>
          </div>
        </section>
      </div>
    </aside>
  </div>
</template>
