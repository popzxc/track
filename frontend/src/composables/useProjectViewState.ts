import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'

import type { ProjectInfo } from '../types/task'

type AppPage = 'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'

interface UseProjectViewStateOptions {
  availableProjects: ComputedRef<ProjectInfo[]>
  closeTaskDrawer: () => void
  currentPage: Ref<AppPage>
  selectedProjectFilter: Ref<string>
}

/**
 * Normalizes project selection across the task queue and project metadata page.
 *
 * Project state is derived from backend data that can appear, disappear, or be
 * renamed as repositories are discovered. This composable keeps the current
 * project filter and the selected project-details panel anchored to real
 * projects so the shell does not accumulate defensive checks in multiple
 * places.
 */
export function useProjectViewState(options: UseProjectViewStateOptions) {
  const selectedProjectDetailsId = ref<string | null>(null)

  const selectedProjectRecord = computed(() =>
    options.availableProjects.value.find((project) => project.canonicalName === options.selectedProjectFilter.value) ?? null,
  )

  const selectedProjectDetails = computed(() =>
    options.availableProjects.value.find((project) => project.canonicalName === selectedProjectDetailsId.value) ?? null,
  )

  const defaultCreateProject = computed(
    () =>
      selectedProjectRecord.value?.canonicalName ??
      options.availableProjects.value[0]?.canonicalName ??
      '',
  )

  function selectProjectDetails(project: ProjectInfo) {
    selectedProjectDetailsId.value = project.canonicalName
    options.currentPage.value = 'projects'
    options.closeTaskDrawer()
  }

  watch(
    options.availableProjects,
    (nextProjects) => {
      if (
        !selectedProjectDetailsId.value ||
        !nextProjects.some((project) => project.canonicalName === selectedProjectDetailsId.value)
      ) {
        selectedProjectDetailsId.value = nextProjects[0]?.canonicalName ?? null
      }

      if (
        options.selectedProjectFilter.value &&
        !nextProjects.some((project) => project.canonicalName === options.selectedProjectFilter.value)
      ) {
        options.selectedProjectFilter.value = ''
      }
    },
    { immediate: true },
  )

  return {
    defaultCreateProject,
    selectProjectDetails,
    selectedProjectDetails,
    selectedProjectDetailsId,
    selectedProjectRecord,
  }
}
