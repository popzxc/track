import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { flushPromises } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'

import ShellSidebar from './ShellSidebar.vue'
import { appRoutes } from '../router'

interface ShellSidebarProps {
  activeRemoteWorkCount: number
  remoteAgentConfigured: boolean
  reviewCount: number
  runnerSetupReady: boolean
  totalProjectCount: number
  visibleTaskCount: number
}

async function mountSidebar(initialPath: string, props: ShellSidebarProps) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: appRoutes,
  })

  await router.push(initialPath)

  const wrapper = mount(ShellSidebar, {
    global: {
      plugins: [router],
    },
    props,
  })

  await router.isReady()

  return { router, wrapper }
}

describe('ShellSidebar', () => {
  it('renders shell counts and navigates through the router', async () => {
    const { router, wrapper } = await mountSidebar('/reviews', {
      activeRemoteWorkCount: 4,
      remoteAgentConfigured: true,
      reviewCount: 7,
      runnerSetupReady: false,
      totalProjectCount: 3,
      visibleTaskCount: 12,
    })

    expect(wrapper.text()).toContain('track')
    expect(wrapper.text()).toContain('setup')
    expect(wrapper.text()).toContain('12')
    expect(wrapper.text()).toContain('7')
    expect(wrapper.text()).toContain('4')
    expect(wrapper.text()).toContain('3')

    await wrapper.get('[data-testid="shell-nav-settings"]').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.name).toBe('settings')
  })

  it('shows the ready badge when the runner shell prelude is configured', async () => {
    const { wrapper } = await mountSidebar('/tasks', {
      activeRemoteWorkCount: 0,
      remoteAgentConfigured: true,
      reviewCount: 0,
      runnerSetupReady: true,
      totalProjectCount: 1,
      visibleTaskCount: 2,
    })

    expect(wrapper.text()).toContain('ready')
    expect(wrapper.get('[data-testid="shell-nav-tasks"]').classes()).toContain('border-aqua/35')
  })
})
