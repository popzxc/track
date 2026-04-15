import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import SettingsPage from './SettingsPage.vue'
import { buildRemoteAgentSettings } from '../testing/factories'

describe('SettingsPage', () => {
  it('emits settings page actions and renders summaries', async () => {
    const wrapper = mount(SettingsPage, {
      props: {
        activeRemoteWorkCount: 0,
        cleaningUpRemoteArtifacts: false,
        cleanupSummary: {
          closedTasksCleaned: 1,
          missingTasksCleaned: 2,
          localDispatchHistoriesRemoved: 3,
          remoteWorktreesRemoved: 4,
          remoteRunDirectoriesRemoved: 5,
        },
        remoteAgentSettings: buildRemoteAgentSettings({
          preferredTool: 'claude',
          reviewFollowUp: {
            enabled: true,
            mainUser: 'octocat',
            defaultReviewPrompt: 'Focus on regressions.',
          },
        }),
        resettingRemoteWorkspace: false,
        resetSummary: {
          workspaceEntriesRemoved: 7,
          registryRemoved: true,
        },
        runnerSetupReady: true,
        shellPreludeHelpText: 'Prelude help.',
      },
    })

    await wrapper.get('[data-testid="edit-runner-setup-button"]').trigger('click')
    await wrapper.get('[data-testid="settings-cleanup-button"]').trigger('click')
    await wrapper.get('[data-testid="settings-reset-button"]').trigger('click')

    expect(wrapper.emitted('request-open-runner-setup')).toEqual([[]])
    expect(wrapper.emitted('request-open-cleanup')).toEqual([[]])
    expect(wrapper.emitted('request-open-reset')).toEqual([[]])
    expect(wrapper.get('[data-testid="cleanup-summary"]').text()).toContain('5')
    expect(wrapper.get('[data-testid="reset-summary"]').text()).toContain('Removed')
    expect(wrapper.text()).toContain('Claude')
  })

  it('disables reset while remote work is active', () => {
    const wrapper = mount(SettingsPage, {
      props: {
        activeRemoteWorkCount: 2,
        cleaningUpRemoteArtifacts: false,
        cleanupSummary: null,
        remoteAgentSettings: buildRemoteAgentSettings(),
        resettingRemoteWorkspace: false,
        resetSummary: null,
        runnerSetupReady: false,
        shellPreludeHelpText: 'Prelude help.',
      },
    })

    expect(wrapper.get('[data-testid="settings-reset-button"]').attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('Stop active task runs and PR reviews before resetting the remote workspace.')
  })

  it('renders opencode in the preferred tool summary', () => {
    const wrapper = mount(SettingsPage, {
      props: {
        activeRemoteWorkCount: 0,
        cleaningUpRemoteArtifacts: false,
        cleanupSummary: null,
        remoteAgentSettings: buildRemoteAgentSettings(
          {},
          { preferredTool: 'opencode' },
        ),
        resettingRemoteWorkspace: false,
        resetSummary: null,
        runnerSetupReady: true,
        shellPreludeHelpText: 'Prelude help.',
      },
    })

    expect(wrapper.text()).toContain('opencode')
  })
})
