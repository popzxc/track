<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import ConfirmDialog from '../components/ConfirmDialog.vue'
import FollowUpModal from '../components/FollowUpModal.vue'
import TaskDrawer from '../components/TaskDrawer.vue'
import TaskEditorModal from '../components/TaskEditorModal.vue'
import TasksPageContent from '../components/TasksPage.vue'
import {
  cancelDispatch,
  createTask,
  deleteTask,
  discardDispatch,
  dispatchTask,
  followUpTask,
  updateTask,
} from '../api/client'
import { upsertTaskRunRecord } from '../composables/useRunState'
import { useTrackerShell } from '../composables/useTrackerShell'
import { taskTitle } from '../features/tasks/description'
import {
  drawerPrimaryAction,
  getRunStartDisabledReason,
  groupTasksByProject,
  type TaskGroup,
} from '../features/tasks/presentation'
import { firstQueryValue, queryFlag, replaceRouteQuery } from '../router/query'
import type {
  RemoteAgentPreferredTool,
  RunRecord,
  Task,
  TaskDispatch,
  TaskFollowUpInput,
} from '../types/task'

type TaskLifecycleMutation = 'closing' | 'reopening' | 'deleting'

const route = useRoute()
const router = useRouter()
const shell = useTrackerShell()

const selectedTaskRuns = ref<RunRecord[]>([])
let selectedTaskRunsRequestVersion = 0

const cancelingDispatchTaskId = ref<string | null>(null)
const discardingDispatchTaskId = ref<string | null>(null)
const dispatchingTaskId = ref<string | null>(null)
const followingUpTaskId = ref<string | null>(null)
const taskLifecycleMutation = ref<TaskLifecycleMutation | null>(null)
const taskLifecycleMutationTaskId = ref<string | null>(null)

const creatingTask = ref(false)
const editingTask = ref<Task | null>(null)
const followingUpTask = ref<Task | null>(null)
const taskPendingDeletion = ref<Task | null>(null)
const selectedTaskStartTool = ref<RemoteAgentPreferredTool>('codex')

const selectedProjectFilter = computed<string>({
  get: () => firstQueryValue(route.query.project) ?? '',
  set: (project) => {
    void replaceRouteQuery(router, route, {
      project: project || null,
    })
  },
})

const showClosed = computed<boolean>({
  get: () => queryFlag(route.query.closed),
  set: (enabled) => {
    void replaceRouteQuery(router, route, {
      closed: enabled ? '1' : null,
    })
  },
})

const selectedTaskId = computed<string | null>({
  get: () => firstQueryValue(route.query.task),
  set: (taskId) => {
    void replaceRouteQuery(router, route, {
      task: taskId,
    })
  },
})

const isTaskDrawerOpen = computed(() => selectedTaskId.value !== null)
const selectedTask = computed(() =>
  shell.tasks.value.find((task) => task.id === selectedTaskId.value) ?? null,
)
const selectedTaskProject = computed(() =>
  selectedTask.value
    ? shell.availableProjects.value.find((project) => project.canonicalName === selectedTask.value?.project) ?? null
    : null,
)
const selectedTaskLatestDispatch = computed(() =>
  selectedTask.value ? shell.latestTaskDispatchesByTaskId.value[selectedTask.value.id] ?? null : null,
)
const selectedTaskPinnedTool = computed<RemoteAgentPreferredTool | null>(
  () => selectedTaskLatestDispatch.value?.preferredTool ?? null,
)
const selectedTaskDispatchTool = computed<RemoteAgentPreferredTool>(
  () => selectedTaskPinnedTool.value ?? selectedTaskStartTool.value,
)
const selectedTaskLatestReusablePullRequest = computed(() =>
  selectedTaskRuns.value.find((run) => Boolean(run.dispatch.pullRequestUrl))?.dispatch.pullRequestUrl
    ?? selectedTaskLatestDispatch.value?.pullRequestUrl
    ?? null,
)
const selectedTaskLifecycleMutation = computed(() =>
  selectedTask.value && taskLifecycleMutationTaskId.value === selectedTask.value.id
    ? taskLifecycleMutation.value
    : null,
)
const selectedTaskDispatchDisabledReason = computed(() =>
  selectedTask.value
    ? getRunStartDisabledReason(
      selectedTask.value,
      shell.availableProjects.value,
      shell.remoteAgentSettings.value,
    )
    : undefined,
)
const selectedTaskCanContinue = computed(() =>
  Boolean(
    selectedTask.value &&
      selectedTaskLatestDispatch.value &&
      !selectedTaskDispatchDisabledReason.value &&
      selectedTaskLatestDispatch.value.status !== 'preparing' &&
      selectedTaskLatestDispatch.value.status !== 'running' &&
      selectedTaskLatestDispatch.value.branchName &&
      selectedTaskLatestDispatch.value.worktreePath,
  ),
)
const selectedTaskCanStartFresh = computed(() =>
  Boolean(
    selectedTask.value &&
      selectedTask.value.status === 'open' &&
      !selectedTaskDispatchDisabledReason.value &&
      selectedTaskLatestDispatch.value &&
      selectedTaskLatestDispatch.value.status !== 'preparing' &&
      selectedTaskLatestDispatch.value.status !== 'running',
  ),
)
const selectedTaskCanDiscardHistory = computed(() =>
  Boolean(
    selectedTask.value &&
      selectedTaskLatestDispatch.value &&
      selectedTaskLatestDispatch.value.status !== 'preparing' &&
      selectedTaskLatestDispatch.value.status !== 'running',
  ),
)
const selectedTaskLifecycleMessage = computed(() => {
  switch (selectedTaskLifecycleMutation.value) {
    case 'closing':
      return 'Closing the task and cleaning up its remote worktree...'
    case 'reopening':
      return 'Reopening the task so you can continue work...'
    case 'deleting':
      return 'Deleting the task and removing its remote artifacts...'
    case null:
      return ''
  }
})
const selectedTaskPrimaryActionDisabled = computed(() =>
  Boolean(
    !selectedTask.value ||
      selectedTaskLifecycleMutation.value !== null ||
      dispatchingTaskId.value === selectedTask.value.id ||
      cancelingDispatchTaskId.value === selectedTask.value.id ||
      followingUpTaskId.value === selectedTask.value.id ||
      (
        selectedTask.value.status === 'open' &&
        selectedTaskLatestDispatch.value?.status !== 'preparing' &&
        selectedTaskLatestDispatch.value?.status !== 'running' &&
        !selectedTaskCanContinue.value &&
        Boolean(selectedTaskDispatchDisabledReason.value)
      ),
  ),
)
const taskGroups = computed<TaskGroup[]>(() => groupTasksByProject(shell.tasks.value))
const defaultCreateProject = computed(
  () =>
    selectedProjectFilter.value ||
    shell.availableProjects.value[0]?.canonicalName ||
    '',
)

function beginTaskLifecycleMutation(taskId: string, mutation: TaskLifecycleMutation) {
  taskLifecycleMutationTaskId.value = taskId
  taskLifecycleMutation.value = mutation
}

function clearTaskLifecycleMutation() {
  taskLifecycleMutationTaskId.value = null
  taskLifecycleMutation.value = null
}

async function selectTask(taskId: string) {
  selectedTaskId.value = taskId
}

async function closeTaskDrawer() {
  selectedTaskRunsRequestVersion += 1
  selectedTaskRuns.value = []
  selectedTaskId.value = null
}

async function loadSelectedTaskRunHistory() {
  const taskId = selectedTaskId.value
  const requestVersion = ++selectedTaskRunsRequestVersion

  if (!taskId) {
    selectedTaskRuns.value = []
    return
  }

  try {
    const taskRuns = await shell.loadTaskRuns(taskId)
    if (requestVersion !== selectedTaskRunsRequestVersion || selectedTaskId.value !== taskId) {
      return
    }

    selectedTaskRuns.value = taskRuns
  } catch (error) {
    if (requestVersion !== selectedTaskRunsRequestVersion || selectedTaskId.value !== taskId) {
      return
    }

    shell.setFriendlyError(error)
  }
}

async function updateTaskStatus(task: Task, status: Task['status']) {
  shell.saving.value = true
  shell.errorMessage.value = ''
  beginTaskLifecycleMutation(task.id, status === 'closed' ? 'closing' : 'reopening')

  try {
    await updateTask(task.id, { status })
    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    shell.saving.value = false
    clearTaskLifecycleMutation()
  }
}

async function saveTaskEdits(payload: { description: string; priority: Task['priority'] }) {
  if (!editingTask.value) {
    return
  }

  shell.saving.value = true
  shell.errorMessage.value = ''

  try {
    await updateTask(editingTask.value.id, payload)
    editingTask.value = null
    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    shell.saving.value = false
  }
}

async function createTaskFromWeb(payload: Parameters<typeof createTask>[0]) {
  shell.saving.value = true
  shell.errorMessage.value = ''

  try {
    const task = await createTask(payload)
    creatingTask.value = false

    await replaceRouteQuery(router, route, {
      closed: null,
      project: task.project,
      task: task.id,
    })

    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    shell.saving.value = false
  }
}

async function confirmDelete() {
  if (!taskPendingDeletion.value) {
    return
  }

  shell.saving.value = true
  shell.errorMessage.value = ''
  beginTaskLifecycleMutation(taskPendingDeletion.value.id, 'deleting')

  try {
    const deletedTaskId = taskPendingDeletion.value.id
    await deleteTask(deletedTaskId)
    taskPendingDeletion.value = null

    if (selectedTaskId.value === deletedTaskId) {
      await closeTaskDrawer()
    }

    removeTaskRuns(deletedTaskId)
    await shell.refreshAll()
  } catch (error) {
    shell.setFriendlyError(error)
  } finally {
    shell.saving.value = false
    clearTaskLifecycleMutation()
  }
}

function upsertSelectedTaskRun(task: Task, dispatch: TaskDispatch) {
  if (selectedTaskId.value !== task.id) {
    return
  }

  selectedTaskRuns.value = upsertTaskRunRecord(selectedTaskRuns.value, task, dispatch)
}

function removeTaskRuns(taskId: string) {
  shell.removeTaskRuns(taskId)

  if (selectedTaskId.value === taskId) {
    selectedTaskRuns.value = []
  }
}

function openRunnerSetup(task: Task, preferredTool: RemoteAgentPreferredTool) {
  void router.push({
    name: 'settings',
    query: {
      modal: 'runner-setup',
      preferredTool,
      resumeTask: task.id,
    },
  })
}

async function startRemoteRun(
  task: Task,
  preferredTool: RemoteAgentPreferredTool = selectedTaskDispatchTool.value,
) {
  if (shell.remoteAgentSettings.value === null) {
    try {
      await shell.loadRemoteAgentSettings()
    } catch {
      // The message below remains the main fallback if the runner settings
      // endpoint is still unavailable after a best-effort refresh.
    }
  }

  if (shell.remoteAgentSettings.value && !shell.remoteAgentSettings.value.configured) {
    shell.errorMessage.value =
      'Remote dispatch is not configured yet. Run `track remote-agent configure --host <host> --user <user> --identity-file ~/.ssh/track_remote_agent` locally first.'
    await router.push({ name: 'settings' })
    return
  }

  if (shell.remoteAgentSettings.value && !shell.runnerSetupReady.value) {
    openRunnerSetup(task, preferredTool)
    return
  }

  dispatchingTaskId.value = task.id
  shell.errorMessage.value = ''

  try {
    const dispatch = await dispatchTask(task.id, { preferredTool })
    shell.upsertRunRecord(task, dispatch)
    shell.upsertLatestTaskDispatch(dispatch)
    upsertSelectedTaskRun(task, dispatch)
  } catch (error) {
    await shell.refreshAll().catch(() => undefined)
    shell.setFriendlyError(error)
  } finally {
    dispatchingTaskId.value = null
  }
}

async function cancelRemoteRun(task: Task) {
  cancelingDispatchTaskId.value = task.id
  shell.errorMessage.value = ''

  try {
    const dispatch = await cancelDispatch(task.id)
    shell.upsertRunRecord(task, dispatch)
    shell.upsertLatestTaskDispatch(dispatch)
    upsertSelectedTaskRun(task, dispatch)
  } catch (error) {
    await shell.refreshAll().catch(() => undefined)
    shell.setFriendlyError(error)
  } finally {
    cancelingDispatchTaskId.value = null
  }
}

async function discardRunHistory(task: Task) {
  discardingDispatchTaskId.value = task.id
  shell.errorMessage.value = ''

  try {
    await discardDispatch(task.id)
    removeTaskRuns(task.id)
  } catch (error) {
    await shell.refreshAll().catch(() => undefined)
    shell.setFriendlyError(error)
  } finally {
    discardingDispatchTaskId.value = null
  }
}

async function submitFollowUp(payload: TaskFollowUpInput) {
  if (!followingUpTask.value) {
    return
  }

  followingUpTaskId.value = followingUpTask.value.id
  shell.errorMessage.value = ''

  try {
    const task = followingUpTask.value
    const dispatch = await followUpTask(task.id, payload)
    shell.upsertRunRecord(task, dispatch)
    shell.upsertLatestTaskDispatch(dispatch)
    upsertSelectedTaskRun(task, dispatch)
    followingUpTask.value = null
    await shell.refreshAll()
  } catch (error) {
    await shell.refreshAll().catch(() => undefined)
    shell.setFriendlyError(error)
  } finally {
    followingUpTaskId.value = null
  }
}

async function handlePrimaryAction() {
  if (!selectedTask.value) {
    return
  }

  const task = selectedTask.value
  const latestDispatch = selectedTaskLatestDispatch.value

  if (task.status === 'closed') {
    await updateTaskStatus(task, 'open')
    return
  }

  if (latestDispatch?.status === 'preparing' || latestDispatch?.status === 'running') {
    await cancelRemoteRun(task)
    return
  }

  if (selectedTaskCanContinue.value) {
    followingUpTask.value = task
    return
  }

  await startRemoteRun(task)
}

function openTaskEditor(task: Task) {
  editingTask.value = task
}

function openNewTaskEditor() {
  creatingTask.value = true
}

function closeTaskEditor() {
  editingTask.value = null
  creatingTask.value = false
}

function closeFollowUpEditor() {
  followingUpTask.value = null
}

function queueTaskDeletion(task: Task) {
  taskPendingDeletion.value = task
}

function clearPendingDeletion() {
  taskPendingDeletion.value = null
}

function openExternal(url: string) {
  window.open(url, '_blank', 'noreferrer')
}

function openSelectedTaskProjectDetails() {
  if (!selectedTaskProject.value) {
    return
  }

  void router.push({
    name: 'projects',
    query: {
      project: selectedTaskProject.value.canonicalName,
    },
  })
}

function drawerPrimaryActionLabel(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return selectedTaskLifecycleMutation.value === 'reopening' ? 'Reopening...' : 'Reopen task'
    case 'cancel':
      return cancelingDispatchTaskId.value === task.id ? 'Canceling...' : 'Cancel run'
    case 'continue':
      return followingUpTaskId.value === task.id ? 'Continuing...' : 'Continue run'
    case 'start':
      return dispatchingTaskId.value === task.id ? 'Starting...' : 'Start agent'
  }
}

function drawerPrimaryActionClass(task: Task, dispatch?: TaskDispatch | null): string {
  switch (drawerPrimaryAction(task, dispatch)) {
    case 'reopen':
      return 'border border-yellow/30 bg-yellow/10 text-yellow hover:bg-yellow/15'
    case 'cancel':
      return 'border border-orange/30 bg-orange/10 text-orange hover:bg-orange/15'
    case 'continue':
      return 'border border-aqua/30 bg-aqua/10 text-aqua hover:bg-aqua/15'
    case 'start':
      return 'border border-blue/30 bg-blue/10 text-blue hover:bg-blue/15'
  }
}

watch(selectedTaskId, () => {
  selectedTaskStartTool.value = shell.defaultRemoteAgentPreferredTool.value
  void loadSelectedTaskRunHistory()
}, { immediate: true })

watch(shell.defaultRemoteAgentPreferredTool, (nextTool, previousTool) => {
  if (selectedTaskStartTool.value === previousTool) {
    selectedTaskStartTool.value = nextTool
  }
})

watch(shell.tasks, () => {
  if (selectedTaskId.value && !selectedTask.value) {
    void closeTaskDrawer()
  }
})

</script>

<template>
  <TasksPageContent
    :active-task-id="selectedTask?.id ?? null"
    :drawer-open="isTaskDrawerOpen"
    :latest-dispatch-by-task-id="shell.latestTaskDispatchesByTaskId.value"
    :projects="shell.availableProjects.value"
    :selected-project-filter="selectedProjectFilter"
    :show-closed="showClosed"
    :task-count="shell.tasks.value.length"
    :task-groups="taskGroups"
    @request-create-task="openNewTaskEditor"
    @request-select-task="selectTask"
    @update:selected-project-filter="selectedProjectFilter = $event"
    @update:show-closed="showClosed = $event"
  />

  <TaskDrawer
    v-if="isTaskDrawerOpen && selectedTask"
    :can-continue="selectedTaskCanContinue"
    :can-discard-history="selectedTaskCanDiscardHistory"
    :can-start-fresh="selectedTaskCanStartFresh"
    :dispatch-disabled-reason="selectedTaskDispatchDisabledReason"
    :is-discarding-history="discardingDispatchTaskId === selectedTask.id"
    :is-dispatching="dispatchingTaskId === selectedTask.id"
    :latest-dispatch="selectedTaskLatestDispatch"
    :latest-reusable-pull-request="selectedTaskLatestReusablePullRequest"
    :lifecycle-mutation="selectedTaskLifecycleMutation"
    :lifecycle-progress-message="selectedTaskLifecycleMessage"
    :pinned-tool="selectedTaskPinnedTool"
    :primary-action-class="drawerPrimaryActionClass(selectedTask, selectedTaskLatestDispatch)"
    :primary-action-disabled="selectedTaskPrimaryActionDisabled"
    :primary-action-label="drawerPrimaryActionLabel(selectedTask, selectedTaskLatestDispatch)"
    :start-tool="selectedTaskDispatchTool"
    :task="selectedTask"
    :task-project="selectedTaskProject"
    :task-runs="selectedTaskRuns"
    @close="closeTaskDrawer"
    @request-close-task="updateTaskStatus(selectedTask, 'closed')"
    @request-delete-task="queueTaskDeletion(selectedTask)"
    @request-discard-history="discardRunHistory(selectedTask)"
    @request-edit-task="openTaskEditor(selectedTask)"
    @request-open-project="openSelectedTaskProjectDetails"
    @request-open-url="openExternal"
    @request-primary-action="handlePrimaryAction"
    @request-start-fresh="startRemoteRun(selectedTask)"
    @update:start-tool="selectedTaskStartTool = $event"
  />

  <TaskEditorModal
    :busy="shell.saving.value"
    :default-project="defaultCreateProject"
    :mode="creatingTask ? 'create' : 'edit'"
    :open="creatingTask || editingTask !== null"
    :projects="shell.availableProjects.value"
    :task="editingTask"
    @cancel="closeTaskEditor"
    @save="creatingTask ? createTaskFromWeb($event) : saveTaskEdits($event)"
  />

  <FollowUpModal
    :busy="followingUpTaskId !== null"
    :dispatch="followingUpTask ? shell.latestTaskDispatchesByTaskId.value[followingUpTask.id] ?? undefined : undefined"
    :open="followingUpTask !== null"
    :task="followingUpTask"
    @cancel="closeFollowUpEditor"
    @save="submitFollowUp"
  />

  <ConfirmDialog
    :busy="shell.saving.value"
    confirm-busy-label="Deleting..."
    confirm-label="Delete forever"
    confirm-variant="danger"
    :description="taskPendingDeletion ? `Delete ${taskTitle(taskPendingDeletion)} permanently? This cannot be undone.` : ''"
    eyebrow="Destructive action"
    :open="taskPendingDeletion !== null"
    title="Delete task"
    @cancel="clearPendingDeletion"
    @confirm="confirmDelete"
  />
</template>
