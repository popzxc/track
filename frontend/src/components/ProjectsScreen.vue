<script setup lang="ts">
import ProjectMetadataModal from './ProjectMetadataModal.vue'
import ProjectsPage from './ProjectsPage.vue'
import type { ProjectsScreenController } from '../composables/useProjectsScreenController'

const props = defineProps<{
  active: boolean
  controller: ProjectsScreenController
}>()

function openProjectEditor(project = props.controller.selectedProjectDetails.value) {
  if (!project) {
    return
  }

  props.controller.editingProject.value = project
}

function closeProjectEditor() {
  props.controller.editingProject.value = null
}
</script>

<template>
  <ProjectsPage
    v-if="active"
    :projects="controller.availableProjects.value"
    :selected-project-details="controller.selectedProjectDetails.value"
    :selected-project-id="controller.selectedProjectDetailsId.value"
    @request-edit-project="openProjectEditor"
    @request-select-project="controller.selectedProjectDetailsId.value = $event"
  />

  <ProjectMetadataModal
    :busy="controller.saving.value"
    :open="controller.editingProject.value !== null"
    :project="controller.editingProject.value"
    @cancel="closeProjectEditor"
    @save="controller.saveProjectEdits"
  />
</template>
