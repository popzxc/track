import { afterEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import RemoteAgentSetupModal from './RemoteAgentSetupModal.vue'
import { buildRemoteAgentSettings } from '../testing/factories'

afterEach(() => {
  document.body.innerHTML = ''
})

describe('RemoteAgentSetupModal', () => {
  it('includes the default review prompt when saving runner settings', async () => {
    const wrapper = mount(RemoteAgentSetupModal, {
      global: {
        stubs: {
          teleport: true,
        },
      },
      props: {
        open: true,
        busy: false,
        settings: buildRemoteAgentSettings({
          reviewFollowUp: {
            enabled: true,
            mainUser: 'octocat',
            defaultReviewPrompt: 'Focus on missing tests.',
          },
        }),
      },
    })

    await wrapper.get('[data-testid="default-review-prompt"]').setValue('Focus on regressions and edge cases.')
    await wrapper.get('[data-testid="save-runner-setup"]').trigger('click')

    expect(wrapper.emitted('save')).toEqual([
      [
        {
          shellPrelude: 'export PATH="/opt/track-testing/bin:$PATH"',
          reviewFollowUp: {
            enabled: true,
            mainUser: 'octocat',
            defaultReviewPrompt: 'Focus on regressions and edge cases.',
          },
        },
      ],
    ])
  })
})
