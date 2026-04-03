<script setup lang="ts">
import type { ProjectInfo } from '../types/task'

const props = defineProps<{
  projects: ProjectInfo[]
  selectedProjectDetails: ProjectInfo | null
  selectedProjectId: string | null
}>()

const emit = defineEmits<{
  'request-edit-project': [project: ProjectInfo]
  'request-select-project': [projectId: string]
}>()
</script>

<template>
  <section class="space-y-4">
    <div class="border border-fg2/20 bg-bg1/95 p-4 shadow-panel">
      <h1 class="font-display text-3xl text-fg0 sm:text-4xl">
        Projects
      </h1>
      <p class="mt-2 text-sm text-fg3">
        Repository metadata for automation
      </p>
    </div>

    <div class="grid gap-4 xl:grid-cols-[minmax(280px,360px)_minmax(0,1fr)]">
      <section class="border border-fg2/20 bg-bg1/95 shadow-panel">
        <div class="border-b border-fg2/10 px-4 py-3">
          <div class="flex items-center justify-between gap-3">
            <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
              Projects
            </p>
            <span class="text-xs text-fg3">{{ projects.length }}</span>
          </div>
        </div>

        <div v-if="projects.length === 0" class="px-4 py-12 text-center">
          <p class="font-display text-2xl text-fg0">
            No projects yet.
          </p>
          <p class="mt-3 text-sm leading-6 text-fg2">
            Projects appear after the CLI registers them with the backend.
          </p>
        </div>

        <div v-else class="divide-y divide-fg2/10">
          <button
            v-for="project in projects"
            :key="project.canonicalName"
            type="button"
            :data-project-id="project.canonicalName"
            data-testid="project-row"
            class="w-full px-4 py-4 text-left transition hover:bg-bg0/40"
            :class="selectedProjectId === project.canonicalName ? 'bg-bg0/55' : 'bg-transparent'"
            @click="emit('request-select-project', project.canonicalName)"
          >
            <p class="text-base text-fg0">
              {{ project.canonicalName }}
            </p>
            <p class="mt-2 text-xs tracking-[0.08em] text-fg3">
              {{ project.metadata?.repoUrl || 'Repository metadata is available through the backend only.' }}
            </p>
          </button>
        </div>
      </section>

      <section class="border border-fg2/20 bg-bg1/95 shadow-panel">
        <div v-if="selectedProjectDetails" class="space-y-6 p-4 sm:p-5">
          <div class="border-b border-fg2/10 pb-4">
            <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
              <div class="min-w-0">
                <h2 class="font-display text-3xl text-fg0 sm:text-4xl">
                  {{ selectedProjectDetails.canonicalName }}
                </h2>
                <p class="mt-3 break-all text-sm leading-7 text-fg2">
                  {{ selectedProjectDetails.metadata?.repoUrl || 'No repository URL has been saved yet.' }}
                </p>
              </div>

              <button
                type="button"
                data-testid="edit-project-button"
                class="border border-aqua/35 bg-aqua/10 px-4 py-3 text-sm font-semibold tracking-[0.08em] text-aqua transition hover:bg-aqua/15"
                @click="emit('request-edit-project', selectedProjectDetails)"
              >
                Edit metadata
              </button>
            </div>
          </div>

          <div class="grid gap-4 xl:grid-cols-2">
            <section class="border border-fg2/15 bg-bg0/60 p-4">
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Repository links
              </p>
              <dl class="mt-4 space-y-4 text-sm">
                <div>
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Repo URL
                  </dt>
                  <dd class="mt-1 break-all text-fg1">
                    {{ selectedProjectDetails.metadata?.repoUrl || 'Not set' }}
                  </dd>
                </div>
                <div>
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Git URL
                  </dt>
                  <dd class="mt-1 break-all text-fg1">
                    {{ selectedProjectDetails.metadata?.gitUrl || 'Not set' }}
                  </dd>
                </div>
                <div>
                  <dt class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                    Base branch
                  </dt>
                  <dd class="mt-1 text-fg1">
                    {{ selectedProjectDetails.metadata?.baseBranch || 'Not set' }}
                  </dd>
                </div>
              </dl>
            </section>

            <section class="border border-fg2/15 bg-bg0/60 p-4">
              <p class="text-[11px] font-semibold uppercase tracking-[0.28em] text-fg3">
                Notes
              </p>
              <div class="mt-4 whitespace-pre-wrap text-sm leading-7 text-fg1">
                {{ selectedProjectDetails.metadata?.description || 'No project description yet.' }}
              </div>
              <div
                v-if="selectedProjectDetails.aliases.length > 0"
                class="mt-4 border-t border-fg2/10 pt-4"
              >
                <p class="text-[11px] font-semibold uppercase tracking-[0.16em] text-fg3">
                  Aliases
                </p>
                <div class="mt-3 flex flex-wrap gap-2 text-[11px] font-semibold tracking-[0.08em]">
                  <span
                    v-for="alias in selectedProjectDetails.aliases"
                    :key="alias"
                    class="border border-fg2/15 bg-bg1 px-2 py-1 text-fg2"
                  >
                    {{ alias }}
                  </span>
                </div>
              </div>
            </section>
          </div>
        </div>

        <div v-else class="flex min-h-[360px] items-center justify-center px-6 py-12 text-center">
          <div>
            <p class="font-display text-2xl text-fg0 sm:text-3xl">
              Select a project
            </p>
            <p class="mt-3 max-w-md text-sm leading-6 text-fg2">
              Project metadata lives here so the queue can stay focused on tasks instead of repository configuration.
            </p>
          </div>
        </div>
      </section>
    </div>
  </section>
</template>
