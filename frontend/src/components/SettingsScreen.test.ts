import { computed, nextTick, ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { shallowMount } from '@vue/test-utils'

import SettingsScreen from './SettingsScreen.vue'
import {
  buildRemoteAgentSettings,
  buildTask,
} from '../testing/factories'

function createContext() {
  return {
    active: true,
    context: {
      activeRemoteWorkCount: computed(() => 1),
      cleaningUpRemoteArtifacts: ref(false),
      cleanupPendingConfirmation: ref(false),
      cleanupSummary: ref(null),
      confirmRemoteCleanup: vi.fn().mockResolvedValue(undefined),
      confirmRemoteReset: vi.fn().mockResolvedValue(undefined),
      editingRemoteAgentSetup: ref(false),
      remoteAgentSettings: ref(buildRemoteAgentSettings()),
      resetPendingConfirmation: ref(false),
      resettingRemoteWorkspace: ref(false),
      resetSummary: ref(null),
      runnerSetupReady: computed(() => true),
      saveRemoteAgentSetup: vi.fn().mockResolvedValue(undefined),
      saving: ref(false),
      shellPreludeHelpText: 'help text',
      taskPendingRunnerSetup: ref({
        preferredTool: 'codex' as const,
        task: buildTask(),
      }),
    },
  }
}

describe('SettingsScreen', () => {
  it('opens the runner setup modal and clears queued task intent', async () => {
    const props = createContext()
    const wrapper = shallowMount(SettingsScreen, {
      props,
    })

    wrapper.findComponent({ name: 'SettingsPage' }).vm.$emit('request-open-runner-setup')
    await nextTick()

    expect(props.context.taskPendingRunnerSetup.value).toBeNull()
    expect(wrapper.findComponent({ name: 'RemoteAgentSetupModal' }).props('open')).toBe(true)
  })

  it('opens maintenance confirmations and forwards confirm actions', async () => {
    const props = createContext()
    const wrapper = shallowMount(SettingsScreen, {
      props,
    })

    wrapper.findComponent({ name: 'SettingsPage' }).vm.$emit('request-open-cleanup')
    wrapper.findComponent({ name: 'SettingsPage' }).vm.$emit('request-open-reset')
    await nextTick()

    const dialogs = wrapper.findAllComponents({ name: 'ConfirmDialog' })
    expect(dialogs[0]?.props('open')).toBe(true)
    expect(dialogs[1]?.props('open')).toBe(true)

    dialogs[0]?.vm.$emit('confirm')
    dialogs[1]?.vm.$emit('confirm')
    await nextTick()

    expect(props.context.confirmRemoteCleanup).toHaveBeenCalledTimes(1)
    expect(props.context.confirmRemoteReset).toHaveBeenCalledTimes(1)
  })
})
