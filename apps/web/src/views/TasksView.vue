<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'

import { ApiClientError, deleteTask, fetchProjects, fetchTasks, updateTask } from '../api/client'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import TaskEditorModal from '../components/TaskEditorModal.vue'
import TaskList from '../components/TaskList.vue'
import Toolbar from '../components/Toolbar.vue'
import type { ProjectInfo, Task } from '../types/task'

const tasks = ref<Task[]>([])
const projects = ref<ProjectInfo[]>([])
const showClosed = ref(false)
const selectedProject = ref('')
const loading = ref(true)
const refreshing = ref(false)
const saving = ref(false)
const errorMessage = ref('')
const editingTask = ref<Task | null>(null)
const taskPendingDeletion = ref<Task | null>(null)

const visibleTaskCount = computed(() => tasks.value.length)

function setFriendlyError(error: unknown) {
  if (error instanceof ApiClientError) {
    errorMessage.value = error.message
    return
  }

  errorMessage.value = error instanceof Error ? error.message : 'Something went wrong while talking to the API.'
}

async function loadProjects() {
  projects.value = await fetchProjects()
}

async function loadTasks() {
  tasks.value = await fetchTasks({
    includeClosed: showClosed.value,
    project: selectedProject.value || undefined,
  })
}

async function refreshAll() {
  errorMessage.value = ''
  refreshing.value = true

  try {
    await Promise.all([loadProjects(), loadTasks()])
  } catch (error) {
    setFriendlyError(error)
  } finally {
    loading.value = false
    refreshing.value = false
  }
}

async function updateTaskStatus(task: Task, status: Task['status']) {
  saving.value = true
  errorMessage.value = ''

  try {
    await updateTask(task.id, { status })
    await loadTasks()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function saveTaskEdits(payload: { description: string; priority: Task['priority'] }) {
  if (!editingTask.value) {
    return
  }

  saving.value = true
  errorMessage.value = ''

  try {
    await updateTask(editingTask.value.id, payload)
    editingTask.value = null
    await loadTasks()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

async function confirmDelete() {
  if (!taskPendingDeletion.value) {
    return
  }

  saving.value = true
  errorMessage.value = ''

  try {
    await deleteTask(taskPendingDeletion.value.id)
    taskPendingDeletion.value = null
    await loadTasks()
  } catch (error) {
    setFriendlyError(error)
  } finally {
    saving.value = false
  }
}

function openEditor(task: Task) {
  editingTask.value = task
}

function closeEditor() {
  editingTask.value = null
}

function selectTaskForDeletion(task: Task) {
  taskPendingDeletion.value = task
}

function clearPendingDeletion() {
  taskPendingDeletion.value = null
}

function setSelectedProject(value: string) {
  selectedProject.value = value
}

function setShowClosed(value: boolean) {
  showClosed.value = value
}

watch([showClosed, selectedProject], () => {
  if (loading.value) {
    return
  }

  void loadTasks().catch(setFriendlyError)
})

onMounted(() => {
  void refreshAll()
})
</script>

<template>
  <main class="min-h-screen px-4 py-6 sm:px-6 lg:px-10">
    <div class="mx-auto max-w-6xl space-y-6">
      <Toolbar
        :busy="refreshing"
        :projects="projects"
        :selected-project="selectedProject"
        :show-closed="showClosed"
        :task-count="visibleTaskCount"
        @refresh="refreshAll"
        @update:selected-project="setSelectedProject"
        @update:show-closed="setShowClosed"
      />

      <TaskList
        :error-message="errorMessage"
        :loading="loading"
        :tasks="tasks"
        @close="updateTaskStatus($event, 'closed')"
        @delete="selectTaskForDeletion"
        @edit="openEditor"
        @reopen="updateTaskStatus($event, 'open')"
      />
    </div>

    <TaskEditorModal
      :busy="saving"
      :open="editingTask !== null"
      :task="editingTask"
      @cancel="closeEditor"
      @save="saveTaskEdits"
    />

    <ConfirmDialog
      :busy="saving"
      :description="'Delete this task permanently? This cannot be undone.'"
      :open="taskPendingDeletion !== null"
      title="Delete this task permanently?"
      @cancel="clearPendingDeletion"
      @confirm="confirmDelete"
    />
  </main>
</template>
