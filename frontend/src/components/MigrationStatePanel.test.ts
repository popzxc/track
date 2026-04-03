import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import MigrationStatePanel from './MigrationStatePanel.vue'

describe('MigrationStatePanel', () => {
  it('renders the migration gate and emits import requests', async () => {
    const wrapper = mount(MigrationStatePanel, {
      props: {
        migrationImportPending: false,
        migrationImportSummary: null,
        migrationRequired: true,
        migrationStatus: {
          state: 'import_required',
          requiresMigration: true,
          canImport: true,
          legacyDetected: true,
          summary: {
            projectsFound: 3,
            aliasesFound: 1,
            tasksFound: 12,
            taskDispatchesFound: 4,
            reviewsFound: 2,
            reviewRunsFound: 5,
            remoteAgentConfigured: false,
          },
          skippedRecords: [
            {
              kind: 'task',
              path: '/tmp/legacy-task.md',
              error: 'Invalid frontmatter.',
            },
          ],
          cleanupCandidates: [],
        },
      },
    })

    expect(wrapper.text()).toContain('Migration required')
    expect(wrapper.text()).toContain('Import legacy track data before using the app')
    expect(wrapper.text()).toContain('/tmp/legacy-task.md')

    await wrapper.get('[data-testid="migration-import-button"]').trigger('click')

    expect(wrapper.emitted('request-import-legacy-data')).toEqual([[]])
  })

  it('renders import success details and cleanup commands', () => {
    const wrapper = mount(MigrationStatePanel, {
      props: {
        migrationImportPending: false,
        migrationImportSummary: {
          importedProjects: 2,
          importedAliases: 1,
          importedTasks: 5,
          importedTaskDispatches: 2,
          importedReviews: 1,
          importedReviewRuns: 3,
          remoteAgentConfigImported: true,
          copiedSecretFiles: [],
          skippedRecords: [],
          cleanupCandidates: [
            { path: '/tmp/legacy-data', reason: 'old directory' },
            { path: '/tmp/legacy.json', reason: 'old config' },
          ],
        },
        migrationRequired: false,
        migrationStatus: null,
      },
    })

    expect(wrapper.text()).toContain('Imported 5 tasks, 2 projects, and 1 reviews')
    expect(wrapper.text()).toContain('Optional legacy cleanup')
    expect(wrapper.text()).toContain('rm -rf /tmp/legacy-data')
    expect(wrapper.text()).toContain('rm -f /tmp/legacy.json')
  })
})
