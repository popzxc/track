import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import ShellSidebar from './ShellSidebar.vue'

describe('ShellSidebar', () => {
  it('renders shell counts and emits navigation requests', async () => {
    const wrapper = mount(ShellSidebar, {
      props: {
        activePage: 'reviews',
        activeRemoteWorkCount: 4,
        remoteAgentConfigured: true,
        reviewCount: 7,
        runnerSetupReady: false,
        totalProjectCount: 3,
        visibleTaskCount: 12,
      },
    })

    expect(wrapper.text()).toContain('track')
    expect(wrapper.text()).toContain('setup')
    expect(wrapper.text()).toContain('12')
    expect(wrapper.text()).toContain('7')
    expect(wrapper.text()).toContain('4')
    expect(wrapper.text()).toContain('3')

    await wrapper.get('[data-testid="shell-nav-tasks"]').trigger('click')
    await wrapper.get('[data-testid="shell-nav-settings"]').trigger('click')

    expect(wrapper.emitted('navigate')).toEqual([['tasks'], ['settings']])
  })

  it('shows the ready badge when the runner shell prelude is configured', () => {
    const wrapper = mount(ShellSidebar, {
      props: {
        activePage: 'tasks',
        activeRemoteWorkCount: 0,
        remoteAgentConfigured: true,
        reviewCount: 0,
        runnerSetupReady: true,
        totalProjectCount: 1,
        visibleTaskCount: 2,
      },
    })

    expect(wrapper.text()).toContain('ready')
    expect(wrapper.get('[data-testid="shell-nav-tasks"]').classes()).toContain('border-aqua/35')
  })
})
