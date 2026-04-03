import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, effectScope, nextTick, ref } from 'vue'

import { useTaskViewState } from './useTaskViewState'
import { buildDispatch, buildProject, buildRunRecord, buildTask } from '../testing/factories'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('useTaskViewState', () => {
  it('replays a pending task selection after opening a run that needs queue filters', async () => {
    const task = buildTask({ status: 'closed' })
    const tasks = ref([] as typeof task[])
    const taskRuns = ref([] as ReturnType<typeof buildRunRecord>[])
    const scope = effectScope()

    const state = scope.run(() =>
      useTaskViewState({
        availableProjects: computed(() => [buildProject({ canonicalName: task.project })]),
        cancelingDispatchTaskId: ref(null),
        currentPage: ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('runs'),
        defaultRemoteAgentPreferredTool: computed(() => 'codex' as const),
        dispatchingTaskId: ref(null),
        followingUpTaskId: ref(null),
        latestDispatchByTaskId: computed(() => ({})),
        loadSelectedTaskRunHistory: vi.fn(async () => undefined),
        remoteAgentSettings: ref(null),
        selectedTaskRuns: taskRuns,
        setFriendlyError: vi.fn(),
        taskLifecycleMutation: ref(null),
        taskLifecycleMutationTaskId: ref(null),
        tasks,
      }),
    )

    if (!state) {
      throw new Error('Expected task view state')
    }

    state.openTaskFromRun(buildRunRecord(task))

    expect(state.pendingSelectedTaskId.value).toBe(task.id)
    expect(state.selectedProjectFilter.value).toBe(task.project)
    expect(state.showClosed.value).toBe(true)

    tasks.value = [task]
    await nextTick()

    expect(state.selectedTaskId.value).toBe(task.id)
    expect(state.pendingSelectedTaskId.value).toBeNull()
    expect(state.isTaskDrawerOpen.value).toBe(true)

    scope.stop()
  })

  it('loads task history for the active drawer selection and clears it when leaving the page', async () => {
    const task = buildTask()
    const loadSelectedTaskRunHistory = vi.fn(async () => undefined)
    const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('tasks')
    const selectedTaskRuns = ref([buildRunRecord(task)])

    const scope = effectScope()
    const state = scope.run(() =>
      useTaskViewState({
        availableProjects: computed(() => [buildProject({ canonicalName: task.project })]),
        cancelingDispatchTaskId: ref(null),
        currentPage,
        defaultRemoteAgentPreferredTool: computed(() => 'codex' as const),
        dispatchingTaskId: ref(null),
        followingUpTaskId: ref(null),
        latestDispatchByTaskId: computed(() => ({
          [task.id]: buildDispatch({ taskId: task.id, project: task.project }),
        })),
        loadSelectedTaskRunHistory,
        remoteAgentSettings: ref(null),
        selectedTaskRuns,
        setFriendlyError: vi.fn(),
        taskLifecycleMutation: ref(null),
        taskLifecycleMutationTaskId: ref(null),
        tasks: ref([task]),
      }),
    )

    if (!state) {
      throw new Error('Expected task view state')
    }

    state.selectTask(task.id)
    await nextTick()

    expect(loadSelectedTaskRunHistory).toHaveBeenCalledTimes(1)

    currentPage.value = 'reviews'
    await nextTick()

    expect(state.isTaskDrawerOpen.value).toBe(false)
    expect(selectedTaskRuns.value).toEqual([])

    scope.stop()
  })
})
