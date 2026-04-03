import { computed, nextTick, ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { shallowMount } from '@vue/test-utils'

import TasksScreen from './TasksScreen.vue'
import {
  buildDispatch,
  buildProject,
  buildRemoteAgentSettings,
  buildTask,
} from '../testing/factories'

function createContext() {
  const task = buildTask()
  const project = buildProject({ canonicalName: task.project })
  const dispatch = buildDispatch({
    taskId: task.id,
    project: task.project,
  })

  const creatingTask = ref(false)
  const editingTask = ref<ReturnType<typeof buildTask> | null>(null)
  const followingUpTask = ref<ReturnType<typeof buildTask> | null>(null)
  const taskPendingDeletion = ref<ReturnType<typeof buildTask> | null>(null)
  const remoteAgentSettings = ref(buildRemoteAgentSettings())

  return {
    active: true,
    controller: {
      availableProjects: computed(() => [project]),
      cancelingDispatchTaskId: ref<string | null>(null),
      closeTaskDrawer: vi.fn(),
      creatingTask,
      currentPage: ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('tasks'),
      defaultCreateProject: computed(() => task.project),
      dispatchingTaskId: ref<string | null>(null),
      discardingDispatchTaskId: ref<string | null>(null),
      editingRemoteAgentSetup: ref(false),
      editingTask,
      errorMessage: ref(''),
      followingUpDispatch: computed(() => dispatch),
      followingUpTask,
      followingUpTaskId: ref<string | null>(null),
      isTaskDrawerOpen: ref(true),
      latestTaskDispatchesByTaskId: ref({ [task.id]: dispatch }),
      loadRemoteAgentSettings: vi.fn().mockResolvedValue(undefined),
      loadRuns: vi.fn().mockResolvedValue(undefined),
      openSelectedTaskProjectDetails: vi.fn(),
      pendingSelectedTaskId: ref<string | null>(null),
      refreshAll: vi.fn().mockResolvedValue(undefined),
      remoteAgentSettings,
      removeTaskRuns: vi.fn(),
      runnerSetupReady: computed(() => true),
      saving: ref(false),
      selectedProjectFilter: ref(task.project),
      selectedTask: computed(() => task),
      selectedTaskCanContinue: computed(() => true),
      selectedTaskCanDiscardHistory: computed(() => true),
      selectedTaskCanStartFresh: computed(() => true),
      selectedTaskDispatchDisabledReason: computed(() => undefined),
      selectedTaskDispatchTool: computed(() => 'codex' as const),
      selectedTaskId: ref(task.id),
      selectedTaskLatestDispatch: computed(() => dispatch),
      selectedTaskLatestReusablePullRequest: computed(() => dispatch.pullRequestUrl ?? null),
      selectedTaskLifecycleMessage: computed(() => ''),
      selectedTaskLifecycleMutation: computed(() => null),
      selectedTaskPinnedTool: computed(() => 'codex' as const),
      selectedTaskPrimaryActionDisabled: computed(() => false),
      selectedTaskProject: computed(() => project),
      selectedTaskRuns: ref([{ task, dispatch }]),
      selectedTaskStartTool: ref<'codex' | 'claude'>('codex'),
      selectTask: vi.fn(),
      setFriendlyError: vi.fn(),
      showClosed: ref(false),
      taskGroups: computed(() => [{ project: task.project, tasks: [task] }]),
      taskLifecycleMutation: ref(null),
      taskLifecycleMutationTaskId: ref<string | null>(null),
      taskPendingDeletion,
      taskPendingRunnerSetup: ref(null),
      tasks: ref([task]),
      upsertLatestTaskDispatch: vi.fn(),
      upsertRunRecord: vi.fn(),
      upsertSelectedTaskRun: vi.fn(),
    },
  }
}

describe('TasksScreen', () => {
  it('opens the task editor when the page requests a new task', async () => {
    const wrapper = shallowMount(TasksScreen, {
      props: createContext(),
    })

    wrapper.findComponent({ name: 'TasksPage' }).vm.$emit('request-create-task')
    await nextTick()

    expect(wrapper.findComponent({ name: 'TaskEditorModal' }).props('open')).toBe(true)
  })

  it('opens the delete confirmation from the drawer', async () => {
    const wrapper = shallowMount(TasksScreen, {
      props: createContext(),
    })

    wrapper.findComponent({ name: 'TaskDrawer' }).vm.$emit('request-delete-task')
    await nextTick()

    expect(wrapper.findComponent({ name: 'ConfirmDialog' }).props('open')).toBe(true)
  })
})
