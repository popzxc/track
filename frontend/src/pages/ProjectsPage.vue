<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import ProjectMetadataModal from '../components/ProjectMetadataModal.vue'
import ProjectsPageContent from '../components/ProjectsPage.vue'
import { updateProject } from '../api/client'
import { useTrackerShell } from '../composables/useTrackerShell'
import { firstQueryValue, replaceRouteQuery } from '../router/query'
import type { ProjectInfo, ProjectMetadataUpdateInput } from '../types/task'

const route = useRoute()
const router = useRouter()
const shell = useTrackerShell()

const editingProject = ref<ProjectInfo | null>(null)

const selectedProjectId = computed<string | null>({
  get: () => firstQueryValue(route.query.project),
  set: (projectId) => {
    void replaceRouteQuery(router, route, { project: projectId })
  },
})

const selectedProjectDetails = computed(() =>
  shell.availableProjects.value.find((project) => project.canonicalName === selectedProjectId.value) ?? null,
)

async function saveProjectEdits(payload: ProjectMetadataUpdateInput) {
  if (!editingProject.value) {
    return
  }

  shell.saving.value = true
  shell.errorMessage.value = ''

  try {
    await updateProject(editingProject.value.canonicalName, payload)
    editingProject.value = null
    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    shell.saving.value = false
  }
}

function openProjectEditor(project = selectedProjectDetails.value) {
  if (!project) {
    return
  }

  editingProject.value = project
}

function closeProjectEditor() {
  editingProject.value = null
}

watch(shell.availableProjects, (projects) => {
  const hasSelectedProject = selectedProjectId.value
    ? projects.some((project) => project.canonicalName === selectedProjectId.value)
    : false

  if (hasSelectedProject) {
    return
  }

  selectedProjectId.value = projects[0]?.canonicalName ?? null
}, { immediate: true })
</script>

<template>
  <ProjectsPageContent
    :projects="shell.availableProjects.value"
    :selected-project-details="selectedProjectDetails"
    :selected-project-id="selectedProjectId"
    @request-edit-project="openProjectEditor"
    @request-select-project="selectedProjectId = $event"
  />

  <ProjectMetadataModal
    :busy="shell.saving.value"
    :open="editingProject !== null"
    :project="editingProject"
    @cancel="closeProjectEditor"
    @save="saveProjectEdits"
  />
</template>
