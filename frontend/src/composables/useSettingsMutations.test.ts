import { afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import * as apiClient from '../api/client'
import { buildRemoteAgentSettings, buildTask } from '../testing/factories'
import { useSettingsMutations } from './useSettingsMutations'

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

function createSettingsMutationHarness() {
  const cleaningUpRemoteArtifacts = ref(false)
  const cleanupPendingConfirmation = ref(false)
  const cleanupSummary = ref(null)
  const editingProject = ref(null)
  const editingRemoteAgentSetup = ref(true)
  const errorMessage = ref('')
  const migrationImportPending = ref(false)
  const migrationImportSummary = ref(null)
  const migrationStatus = ref({
    state: 'import_required' as const,
    requiresMigration: true,
    canImport: true,
    legacyDetected: true,
    summary: {
      projectsFound: 1,
      aliasesFound: 0,
      tasksFound: 2,
      taskDispatchesFound: 0,
      reviewsFound: 3,
      reviewRunsFound: 0,
      remoteAgentConfigured: false,
    },
    skippedRecords: [],
    cleanupCandidates: [],
  })
  const remoteAgentSettings = ref(null)
  const resetPendingConfirmation = ref(false)
  const resetSummary = ref(null)
  const resettingRemoteWorkspace = ref(false)
  const saving = ref(false)
  const taskPendingRunnerSetup = ref<{
    task: ReturnType<typeof buildTask>
    preferredTool: 'codex' | 'claude'
  } | null>(null)

  const refreshAll = vi.fn(async () => undefined)
  const resumeQueuedTaskDispatch = vi.fn()
  const setFriendlyError = vi.fn()

  return {
    editingRemoteAgentSetup,
    migrationImportPending,
    migrationImportSummary,
    migrationStatus,
    refreshAll,
    remoteAgentSettings,
    resumeQueuedTaskDispatch,
    taskPendingRunnerSetup,
    mutations: useSettingsMutations({
      cleaningUpRemoteArtifacts,
      cleanupPendingConfirmation,
      cleanupSummary,
      editingProject,
      editingRemoteAgentSetup,
      errorMessage,
      migrationImportPending,
      migrationImportSummary,
      migrationStatus,
      refreshAll,
      remoteAgentSettings,
      resetPendingConfirmation,
      resetSummary,
      resettingRemoteWorkspace,
      resumeQueuedTaskDispatch,
      saving,
      setFriendlyError,
      taskPendingRunnerSetup,
    }),
  }
}

describe('useSettingsMutations', () => {
  it('saves remote runner settings and resumes the queued dispatch intent', async () => {
    vi.useFakeTimers()
    const harness = createSettingsMutationHarness()
    const queuedTask = buildTask()
    const savedSettings = buildRemoteAgentSettings({
      preferredTool: 'claude',
      shellPrelude: 'export PATH=/srv/tools:$PATH',
    })
    harness.taskPendingRunnerSetup.value = {
      task: queuedTask,
      preferredTool: 'claude',
    }
    vi.spyOn(apiClient, 'updateRemoteAgentSettings').mockResolvedValue(savedSettings)

    await harness.mutations.saveRemoteAgentSetup({
      preferredTool: 'claude',
      shellPrelude: 'export PATH=/srv/tools:$PATH',
    })
    await vi.runAllTimersAsync()

    expect(harness.remoteAgentSettings.value).toEqual(savedSettings)
    expect(harness.editingRemoteAgentSetup.value).toBe(false)
    expect(harness.taskPendingRunnerSetup.value).toBeNull()
    expect(harness.resumeQueuedTaskDispatch).toHaveBeenCalledWith(queuedTask, 'claude')
  })

  it('imports legacy data and clears the migration gate after success', async () => {
    const harness = createSettingsMutationHarness()
    const summary = {
      importedProjects: 2,
      importedAliases: 0,
      importedTasks: 5,
      importedTaskDispatches: 0,
      importedReviews: 1,
      importedReviewRuns: 0,
      remoteAgentConfigImported: false,
      copiedSecretFiles: [],
      skippedRecords: [],
      cleanupCandidates: [],
    }
    vi.spyOn(apiClient, 'importLegacyData').mockResolvedValue(summary)

    await harness.mutations.importLegacyTrackerData()

    expect(harness.migrationImportSummary.value).toEqual(summary)
    expect(harness.migrationStatus.value).toBeNull()
    expect(harness.migrationImportPending.value).toBe(false)
    expect(harness.refreshAll).toHaveBeenCalledTimes(1)
  })
})
