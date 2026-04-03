import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import * as apiClient from '../api/client'
import { buildProject, buildRemoteAgentSettings, buildTask } from '../testing/factories'
import type {
  MigrationStatus,
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
  const migrationStatus = ref<MigrationStatus | null>(null)
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
    migrationStatus,
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
      migrationStatus,
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

  it('loads the migration gate and clears volatile shell data when refresh is blocked', async () => {
    const harness = createLoaderHarness()
    harness.tasks.value = [buildTask()]
    harness.reviews.value = [{} as ReviewSummary]
    harness.projects.value = [buildProject()]
    harness.runs.value = [{} as RunRecord]
    harness.selectedTaskRuns.value = [{} as RunRecord]
    harness.selectedReviewRuns.value = [{} as ReviewRunRecord]
    harness.latestTaskDispatchesByTaskId.value = { existing: {} as TaskDispatch }
    harness.remoteAgentSettings.value = buildRemoteAgentSettings()

    const migrationStatus = {
      state: 'import_required' as const,
      requiresMigration: true,
      canImport: true,
      legacyDetected: true,
      summary: {
        projectsFound: 1,
        aliasesFound: 0,
        tasksFound: 1,
        taskDispatchesFound: 0,
        reviewsFound: 0,
        reviewRunsFound: 0,
        remoteAgentConfigured: false,
      },
      skippedRecords: [],
      cleanupCandidates: [],
    }

    vi.spyOn(apiClient, 'fetchProjects').mockRejectedValue(
      new apiClient.ApiClientError('MIGRATION_REQUIRED', 'Import required.'),
    )
    vi.spyOn(apiClient, 'fetchTasks').mockResolvedValue([])
    vi.spyOn(apiClient, 'fetchMigrationStatus').mockResolvedValue(migrationStatus)
    vi.spyOn(apiClient, 'fetchRemoteAgentSettings').mockResolvedValue(buildRemoteAgentSettings())

    await harness.loader.refreshAll()

    expect(harness.migrationStatus.value).toEqual(migrationStatus)
    expect(harness.tasks.value).toEqual([])
    expect(harness.reviews.value).toEqual([])
    expect(harness.projects.value).toEqual([])
    expect(harness.runs.value).toEqual([])
    expect(harness.selectedTaskRuns.value).toEqual([])
    expect(harness.selectedReviewRuns.value).toEqual([])
    expect(harness.latestTaskDispatchesByTaskId.value).toEqual({})
    expect(harness.remoteAgentSettings.value).toBeNull()
    expect(harness.setFriendlyError).not.toHaveBeenCalled()
    expect(harness.loading.value).toBe(false)
    expect(harness.refreshing.value).toBe(false)
  })
})
