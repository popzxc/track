import type { ComputedRef, Ref } from 'vue'

import type {
  ProjectInfo,
  ProjectMetadataUpdateInput,
} from '../types/task'

interface UseProjectsScreenControllerOptions {
  availableProjects: ComputedRef<ProjectInfo[]>
  editingProject: Ref<ProjectInfo | null>
  saveProjectEdits: (payload: ProjectMetadataUpdateInput) => Promise<void>
  saving: Ref<boolean>
  selectedProjectDetails: ComputedRef<ProjectInfo | null>
  selectedProjectDetailsId: Ref<string | null>
}

/**
 * Marks the project metadata page as its own named boundary.
 *
 * Project editing is simpler than task or review orchestration, but keeping a
 * dedicated controller still matters because project details are selected from
 * one part of the shell and edited from another. Naming that boundary keeps the
 * screen readable without pushing those coordination details back into App.vue.
 */
export function useProjectsScreenController(options: UseProjectsScreenControllerOptions) {
  return {
    availableProjects: options.availableProjects,
    editingProject: options.editingProject,
    saveProjectEdits: options.saveProjectEdits,
    saving: options.saving,
    selectedProjectDetails: options.selectedProjectDetails,
    selectedProjectDetailsId: options.selectedProjectDetailsId,
  }
}

export type ProjectsScreenController = ReturnType<typeof useProjectsScreenController>
