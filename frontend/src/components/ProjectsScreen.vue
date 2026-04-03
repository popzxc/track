<script setup lang="ts">
import type { ComputedRef, Ref } from 'vue'

import ProjectMetadataModal from './ProjectMetadataModal.vue'
import ProjectsPage from './ProjectsPage.vue'
import type {
  ProjectInfo,
  ProjectMetadataUpdateInput,
} from '../types/task'

interface ProjectsScreenContext {
  availableProjects: ComputedRef<ProjectInfo[]>
  editingProject: Ref<ProjectInfo | null>
  saveProjectEdits: (payload: ProjectMetadataUpdateInput) => Promise<void>
  saving: Ref<boolean>
  selectedProjectDetails: ComputedRef<ProjectInfo | null>
  selectedProjectDetailsId: Ref<string | null>
}

const props = defineProps<{
  active: boolean
  context: ProjectsScreenContext
}>()

// Project metadata editing is a page-scoped workflow rather than generic shell
// chrome. Keeping the page and its editor modal together makes that ownership
// explicit before we tackle the larger controller split.
function openProjectEditor(project = props.context.selectedProjectDetails.value) {
  if (!project) {
    return
  }

  props.context.editingProject.value = project
}

function closeProjectEditor() {
  props.context.editingProject.value = null
}
</script>

<template>
  <ProjectsPage
    v-if="active"
    :projects="context.availableProjects.value"
    :selected-project-details="context.selectedProjectDetails.value"
    :selected-project-id="context.selectedProjectDetailsId.value"
    @request-edit-project="openProjectEditor"
    @request-select-project="context.selectedProjectDetailsId.value = $event"
  />

  <ProjectMetadataModal
    :busy="context.saving.value"
    :open="context.editingProject.value !== null"
    :project="context.editingProject.value"
    @cancel="closeProjectEditor"
    @save="context.saveProjectEdits"
  />
</template>
