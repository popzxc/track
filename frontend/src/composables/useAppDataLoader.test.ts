import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import * as apiClient from '../api/client'
import { buildProject, buildRemoteAgentSettings, buildTask } from '../testing/factories'
import type {
  ProjectInfo,
  RemoteAgentSettings,
  ReviewRunRecord,
  ReviewSummary,
  RunRecord,
  Task,
  TaskDispatch,
} from '../types/task'
import { useAppDataLoader } from './useAppDataLoader'

afterEach(() => {
  vi.restoreAllMocks()
})

function createLoaderHarness() {
  const errorMessage = ref('stale error')
  const latestTaskDispatchesByTaskId = ref<Record<string, TaskDispatch>>({})
  const loading = ref(true)
  const projects = ref<ProjectInfo[]>([])
  const refreshing = ref(false)
  const remoteAgentSettings = ref<RemoteAgentSettings | null>(null)
  const reviews = ref<ReviewSummary[]>([])
  const runs = ref<RunRecord[]>([])
  const selectedProjectFilter = ref('')
  const selectedReviewRuns = ref<ReviewRunRecord[]>([])
  const selectedTaskRuns = ref<RunRecord[]>([])
  const showClosed = ref(false)
  const taskProjectOptions = ref<ProjectInfo[]>([])
  const tasks = ref<Task[]>([])

  const loadLatestDispatchesForVisibleTasks = vi.fn(async () => undefined)
  const loadReviews = vi.fn(async () => undefined)
  const loadRuns = vi.fn(async () => undefined)
  const loadSelectedReviewRunHistory = vi.fn(async () => undefined)
  const loadSelectedTaskRunHistory = vi.fn(async () => undefined)
  const setFriendlyError = vi.fn()
  const syncTaskChangeVersion = vi.fn(async () => undefined)

  return {
    errorMessage,
    latestTaskDispatchesByTaskId,
    loading,
    loadLatestDispatchesForVisibleTasks,
    loadReviews,
    loadRuns,
    loadSelectedReviewRunHistory,
    loadSelectedTaskRunHistory,
    projects,
    refreshing,
    remoteAgentSettings,
    reviews,
    runs,
    selectedReviewRuns,
    selectedTaskRuns,
    setFriendlyError,
    syncTaskChangeVersion,
    taskProjectOptions,
    tasks,
    loader: useAppDataLoader({
      errorMessage,
      latestTaskDispatchesByTaskId,
      loading,
      loadLatestDispatchesForVisibleTasks,
      loadReviews,
      loadRuns,
      loadSelectedReviewRunHistory,
      loadSelectedTaskRunHistory,
      projects,
      refreshing,
      remoteAgentSettings,
      reviews,
      runs,
      selectedProjectFilter,
      selectedReviewRuns,
      selectedTaskRuns,
      setFriendlyError,
      showClosed,
      syncTaskChangeVersion,
      taskProjectOptions,
      tasks,
    }),
  }
}

describe('useAppDataLoader', () => {
  it('refreshes the shell data slices and rebuilds task project options', async () => {
    const harness = createLoaderHarness()
    const tasks = [
      buildTask({ project: 'project-a' }),
      buildTask({
        id: 'project-b/open/20260323-120500-another-task.md',
        project: 'project-b',
      }),
    ]

    vi.spyOn(apiClient, 'fetchProjects').mockResolvedValue([
      buildProject({ canonicalName: 'project-a' }),
      buildProject({ canonicalName: 'project-b' }),
    ])
    const fetchTasksSpy = vi.spyOn(apiClient, 'fetchTasks').mockResolvedValue(tasks)
    vi.spyOn(apiClient, 'fetchRemoteAgentSettings').mockResolvedValue(buildRemoteAgentSettings())

    await harness.loader.refreshAll()

    expect(fetchTasksSpy).toHaveBeenCalledWith({ includeClosed: false, project: undefined })
    expect(harness.projects.value.map((project) => project.canonicalName)).toEqual(['project-a', 'project-b'])
    expect(harness.tasks.value.map((task) => task.project)).toEqual(['project-a', 'project-b'])
    expect(harness.taskProjectOptions.value.map((project) => project.canonicalName)).toEqual(['project-a', 'project-b'])
    expect(harness.remoteAgentSettings.value?.configured).toBe(true)
    expect(harness.loadReviews).toHaveBeenCalledTimes(1)
    expect(harness.loadLatestDispatchesForVisibleTasks).toHaveBeenCalledTimes(1)
    expect(harness.loadRuns).toHaveBeenCalledTimes(1)
    expect(harness.loadSelectedTaskRunHistory).toHaveBeenCalledTimes(1)
    expect(harness.loadSelectedReviewRunHistory).toHaveBeenCalledTimes(1)
    expect(harness.syncTaskChangeVersion).toHaveBeenCalledTimes(1)
    expect(harness.errorMessage.value).toBe('')
    expect(harness.loading.value).toBe(false)
    expect(harness.refreshing.value).toBe(false)
  })

  it('clears the initial loading gate before the slow second-wave refresh finishes', async () => {
    const harness = createLoaderHarness()
    let resolveRuns: (() => void) | null = null

    vi.spyOn(apiClient, 'fetchProjects').mockResolvedValue([buildProject()])
    vi.spyOn(apiClient, 'fetchTasks').mockResolvedValue([buildTask()])
    vi.spyOn(apiClient, 'fetchRemoteAgentSettings').mockResolvedValue(buildRemoteAgentSettings())
    harness.loadRuns.mockImplementation(
      () =>
        new Promise<undefined>((resolve) => {
          resolveRuns = () => resolve(undefined)
        }),
    )

    const refreshPromise = harness.loader.refreshAll()
    await vi.waitFor(() => {
      expect(harness.loadRuns).toHaveBeenCalledTimes(1)
    })

    expect(harness.loading.value).toBe(false)
    expect(harness.refreshing.value).toBe(true)

    expect(resolveRuns).not.toBeNull()
    resolveRuns!()
    await refreshPromise

    expect(harness.refreshing.value).toBe(false)
  })
})
