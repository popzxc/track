import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, effectScope, ref } from 'vue'

import * as apiClient from '../api/client'
import {
  buildDispatch,
  buildProject,
  buildRemoteAgentSettings,
  buildReviewSummary,
  buildRunRecord,
  buildTask,
} from '../testing/factories'
import { useWorkflowScreens } from './useWorkflowScreens'

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

function createWorkflowHarness() {
  const currentPage = ref<'tasks' | 'reviews' | 'runs' | 'projects' | 'settings'>('tasks')
  const errorMessage = ref('')
  const remoteAgentSettings = ref(buildRemoteAgentSettings())
  const saving = ref(false)
  const tasks = ref([
    buildTask(),
    buildTask({
      id: 'project-b/open/20260323-130000-follow-up-history.md',
      project: 'project-b',
    }),
  ])
  const reviews = ref([
    buildReviewSummary(),
  ])
  const runs = ref([])
  const availableProjects = ref([
    buildProject({ canonicalName: 'project-a' }),
    buildProject({ canonicalName: 'project-b' }),
  ])
  const setFriendlyError = vi.fn()
  const refreshAll = vi.fn(async () => undefined)
  const loadRemoteAgentSettings = vi.fn(async () => undefined)

  const scope = effectScope()
  const workflow = scope.run(() =>
    useWorkflowScreens({
      availableProjects: computed(() => availableProjects.value),
      canRequestReview: computed(() => true),
      currentPage,
      defaultRemoteAgentPreferredTool: computed(() => remoteAgentSettings.value?.preferredTool ?? 'codex'),
      errorMessage,
      remoteAgentSettings,
      reviewRequestDisabledReason: computed(() => undefined),
      runnerSetupReady: computed(() => Boolean(remoteAgentSettings.value?.shellPrelude?.trim())),
      saving,
      setFriendlyError,
      shellPreludeHelpText: 'Runner setup help text',
      tasks,
      reviews,
      runs,
    }),
  )

  if (!workflow) {
    throw new Error('Expected workflow screens')
  }

  workflow.connectDataLoader({
    loadRemoteAgentSettings,
    refreshAll,
  })

  return {
    availableProjects,
    currentPage,
    loadRemoteAgentSettings,
    refreshAll,
    remoteAgentSettings,
    reviews,
    runs,
    scope,
    setFriendlyError,
    tasks,
    workflow,
  }
}

describe('useWorkflowScreens', () => {
  it('routes runner setup requests into Settings with queued task context', () => {
    const harness = createWorkflowHarness()
    const task = harness.tasks.value[0]

    harness.workflow.tasksScreen.requestRunnerSetup(task, 'claude')

    expect(harness.currentPage.value).toBe('settings')
    expect(harness.workflow.settingsScreen.editingRemoteAgentSetup.value).toBe(true)
    expect(harness.workflow.settingsScreen.taskPendingRunnerSetup.value).toEqual({
      task,
      preferredTool: 'claude',
    })

    harness.scope.stop()
  })

  it('saves runner setup and resumes the queued dispatch through the queue integration', async () => {
    vi.useFakeTimers()
    const harness = createWorkflowHarness()
    const task = harness.tasks.value[0]
    const dispatch = buildDispatch({
      dispatchId: 'dispatch-resumed',
      taskId: task.id,
      project: task.project,
      preferredTool: 'claude',
      status: 'running',
    })

    harness.workflow.tasksScreen.requestRunnerSetup(task, 'claude')
    vi.spyOn(apiClient, 'updateRemoteAgentSettings').mockResolvedValue(
      buildRemoteAgentSettings({
        preferredTool: 'claude',
        shellPrelude: 'export PATH=/srv/tools:$PATH',
      }),
    )
    const dispatchTaskSpy = vi.spyOn(apiClient, 'dispatchTask').mockResolvedValue(dispatch)

    await harness.workflow.settingsScreen.saveRemoteAgentSetup({
      preferredTool: 'claude',
      shellPrelude: 'export PATH=/srv/tools:$PATH',
    })
    await vi.runAllTimersAsync()

    expect(dispatchTaskSpy).toHaveBeenCalledWith(task.id, { preferredTool: 'claude' })
    expect(harness.workflow.settingsScreen.editingRemoteAgentSetup.value).toBe(false)
    expect(harness.workflow.settingsScreen.taskPendingRunnerSetup.value).toBeNull()
    expect(harness.workflow.runsScreen.recentRuns.value[0]?.dispatch.dispatchId).toBe('dispatch-resumed')

    harness.scope.stop()
  })

  it('opens project details from the selected task and closes the drawer', () => {
    const harness = createWorkflowHarness()
    const task = harness.tasks.value[1]

    harness.workflow.tasksScreen.selectTask(task.id)
    harness.workflow.tasksScreen.openSelectedTaskProjectDetails()

    expect(harness.currentPage.value).toBe('projects')
    expect(harness.workflow.projectsScreen.selectedProjectDetailsId.value).toBe(task.project)
    expect(harness.workflow.tasksScreen.isTaskDrawerOpen.value).toBe(false)

    harness.scope.stop()
  })

  it('loads task and review history into the screen-owned run collections', async () => {
    const harness = createWorkflowHarness()
    const task = harness.tasks.value[0]
    const review = harness.reviews.value[0]?.review

    if (!review) {
      throw new Error('Expected review fixture')
    }

    const taskRun = buildRunRecord(
      { id: task.id, project: task.project },
      { dispatchId: 'task-history-1', taskId: task.id, project: task.project },
    )
    const reviewRun = {
      ...harness.reviews.value[0]!.latestRun!,
      dispatchId: 'review-history-1',
      reviewId: review.id,
    }

    vi.spyOn(apiClient, 'fetchTaskRuns').mockResolvedValue([taskRun])
    vi.spyOn(apiClient, 'fetchReviewRuns').mockResolvedValue([reviewRun])

    harness.workflow.tasksScreen.selectTask(task.id)
    await harness.workflow.loadSelectedTaskRunHistory()

    expect(harness.workflow.tasksScreen.selectedTaskRuns.value).toEqual([taskRun])

    harness.workflow.reviewsScreen.selectReview(review.id)
    await harness.workflow.loadSelectedReviewRunHistory()

    expect(harness.workflow.reviewsScreen.selectedReviewRuns.value).toEqual([reviewRun])

    harness.scope.stop()
  })
})
