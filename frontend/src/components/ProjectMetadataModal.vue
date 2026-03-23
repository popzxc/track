<script setup lang="ts">
import { ref, watch } from 'vue'

import type { ProjectInfo, ProjectMetadataUpdateInput } from '../types/task'

const props = defineProps<{
  busy?: boolean
  open: boolean
  project: ProjectInfo | null
}>()

const emit = defineEmits<{
  cancel: []
  save: [payload: ProjectMetadataUpdateInput]
}>()

const repoUrl = ref('')
const gitUrl = ref('')
const baseBranch = ref('main')
const description = ref('')

watch(
  () => props.project,
  (project) => {
    repoUrl.value = project?.metadata?.repoUrl ?? ''
    gitUrl.value = project?.metadata?.gitUrl ?? ''
    baseBranch.value = project?.metadata?.baseBranch ?? 'main'
    description.value = project?.metadata?.description ?? ''
  },
  { immediate: true },
)

function submit() {
  emit('save', {
    repoUrl: repoUrl.value.trim(),
    gitUrl: gitUrl.value.trim(),
    baseBranch: baseBranch.value.trim(),
    description: description.value.trim() || undefined,
  })
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-bg0/80 px-4 py-6"
    >
      <div class="w-full max-w-3xl border border-fg2/20 bg-bg1 p-6 shadow-panel">
        <div class="flex items-start justify-between gap-4 border-b border-fg2/10 pb-4">
          <div>
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Project metadata
            </p>
            <h3 class="mt-2 font-display text-2xl text-fg0 sm:text-3xl">
              {{ project?.canonicalName ?? 'Project' }}
            </h3>
            <p v-if="project?.path" class="mt-3 text-xs tracking-[0.08em] text-fg3">
              {{ project.path }}
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

        <div class="mt-6 grid gap-5 md:grid-cols-2">
          <label class="block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Repo URL
            <input
              v-model="repoUrl"
              type="text"
              class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
              placeholder="https://github.com/acme/project"
            />
          </label>

          <label class="block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
            Git URL
            <input
              v-model="gitUrl"
              type="text"
              class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
              placeholder="git@github.com:acme/project.git"
            />
          </label>
        </div>

        <label class="mt-5 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
          Base branch
          <input
            v-model="baseBranch"
            type="text"
            class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
            placeholder="main"
          />
        </label>

        <label class="mt-5 block text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
          Description
          <textarea
            v-model="description"
            rows="8"
            class="mt-2 w-full border border-fg2/20 bg-bg0 px-4 py-3 text-sm leading-6 text-fg0 outline-none transition hover:border-fg2/40 focus:border-aqua/50 focus:ring-1 focus:ring-aqua/50"
            placeholder="Optional notes about what this repository is for."
          />
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
            :disabled="busy || repoUrl.trim().length === 0 || gitUrl.trim().length === 0 || baseBranch.trim().length === 0"
            @click="submit"
          >
            {{ busy ? 'Saving...' : 'Save metadata' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
