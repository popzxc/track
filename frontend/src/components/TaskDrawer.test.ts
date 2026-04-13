import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import TaskDrawer from './TaskDrawer.vue'
import { buildDispatch, buildProject, buildRunRecord, buildTask } from '../testing/factories'

describe('TaskDrawer', () => {
  it('emits user actions while the task is still choosing a runner', async () => {
    const task = buildTask()
    const latestDispatch = buildDispatch({
      preferredTool: 'codex',
      status: 'succeeded',
    })

    const wrapper = mount(TaskDrawer, {
      props: {
        canContinue: true,
        canDiscardHistory: true,
        canStartFresh: true,
        isDispatching: false,
        isDiscardingHistory: false,
        latestDispatch,
        latestReusablePullRequest: latestDispatch.pullRequestUrl ?? null,
        lifecycleMutation: null,
        lifecycleProgressMessage: '',
        pinnedTool: null,
        primaryActionClass: 'border border-aqua/30 bg-aqua/10 text-aqua',
        primaryActionDisabled: false,
        primaryActionLabel: 'Continue run',
        startTool: 'codex',
        task,
        taskProject: buildProject({ canonicalName: task.project }),
        taskRuns: [buildRunRecord(task, latestDispatch)],
      },
    })

    await wrapper.get('[data-testid="drawer-dispatch-tool"]').setValue('claude')
    await wrapper.get('[data-testid="drawer-primary-action"]').trigger('click')

    const projectButton = wrapper.findAll('button').find((button) => button.text() === task.project)
    await projectButton?.trigger('click')

    const viewPrButton = wrapper.findAll('button').find((button) => button.text() === 'View PR')
    await viewPrButton?.trigger('click')

    expect(wrapper.emitted('update:startTool')).toEqual([['claude']])
    expect(wrapper.emitted('request-primary-action')).toEqual([[]])
    expect(wrapper.emitted('request-open-project')).toEqual([[]])
    expect(wrapper.emitted('request-open-url')).toEqual([[latestDispatch.pullRequestUrl]])
  })

  it('renders pinned runner context and run history badges', () => {
    const task = buildTask()
    const latestDispatch = buildDispatch({
      preferredTool: 'claude',
      followUpRequest: 'Address the last round of review comments.',
    })

    const wrapper = mount(TaskDrawer, {
      props: {
        canContinue: true,
        canDiscardHistory: true,
        canStartFresh: true,
        isDispatching: false,
        isDiscardingHistory: false,
        latestDispatch,
        latestReusablePullRequest: latestDispatch.pullRequestUrl ?? null,
        lifecycleMutation: null,
        lifecycleProgressMessage: '',
        pinnedTool: 'claude',
        primaryActionClass: 'border border-aqua/30 bg-aqua/10 text-aqua',
        primaryActionDisabled: false,
        primaryActionLabel: 'Continue run',
        startTool: 'claude',
        task,
        taskProject: buildProject({ canonicalName: task.project }),
        taskRuns: [buildRunRecord(task, latestDispatch)],
      },
    })

    expect(wrapper.get('[data-testid="drawer-pinned-tool"]').text()).toContain('Claude')
    expect(wrapper.get('[data-testid="run-latest-badge"]').text()).toBe('Latest')
    expect(wrapper.text()).toContain('Follow-up request')
  })
})
