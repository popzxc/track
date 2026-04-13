import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import TasksPage from './TasksPage.vue'
import { buildDispatch, buildProject, buildTask } from '../testing/factories'
import type { TaskGroup } from '../features/tasks/presentation'

describe('TasksPage', () => {
  it('emits filter and selection events while rendering grouped tasks', async () => {
    const firstTask = buildTask()
    const secondTask = buildTask({
      id: 'project-b/open/20260323-120100-review-run-history.md',
      project: 'project-b',
      description: 'Review run history\n\n## Summary\nMake latest runs easy to spot.',
      priority: 'medium',
    })

    const wrapper = mount(TasksPage, {
      props: {
        activeTaskId: firstTask.id,
        drawerOpen: true,
        latestDispatchByTaskId: {
          [firstTask.id]: buildDispatch({
            taskId: firstTask.id,
            project: firstTask.project,
            status: 'succeeded',
          }),
          [secondTask.id]: buildDispatch({
            dispatchId: 'dispatch-456',
            taskId: secondTask.id,
            project: secondTask.project,
            status: 'running',
            finishedAt: undefined,
          }),
        },
        projects: [
          buildProject({ canonicalName: 'project-a' }),
          buildProject({ canonicalName: 'project-b' }),
        ],
        selectedProjectFilter: '',
        showClosed: false,
        taskCount: 2,
        taskGroups: [
          { project: 'project-a', tasks: [firstTask] },
          { project: 'project-b', tasks: [secondTask] },
        ] satisfies TaskGroup[],
      },
    })

    expect(wrapper.findAll('[data-testid="task-group"]').map((group) => group.attributes('data-project'))).toEqual([
      'project-a',
      'project-b',
    ])
    expect(wrapper.get(`[data-task-id="${firstTask.id}"]`).classes()).toContain('bg-bg0/55')

    await wrapper.get('[data-testid="task-project-filter"]').setValue('project-b')
    await wrapper.get('[data-testid="task-show-closed"]').setValue(true)
    await wrapper.get('[data-testid="new-task-button"]').trigger('click')
    await wrapper.get(`[data-task-id="${secondTask.id}"]`).trigger('click')

    expect(wrapper.emitted('update:selectedProjectFilter')).toEqual([['project-b']])
    expect(wrapper.emitted('update:showClosed')).toEqual([[true]])
    expect(wrapper.emitted('request-create-task')).toEqual([[]])
    expect(wrapper.emitted('request-select-task')).toEqual([[secondTask.id]])
    expect(wrapper.text()).toContain('Agent running')
  })

  it('shows the empty queue state when no tasks are visible', () => {
    const wrapper = mount(TasksPage, {
      props: {
        activeTaskId: null,
        drawerOpen: false,
        latestDispatchByTaskId: {},
        projects: [buildProject()],
        selectedProjectFilter: '',
        showClosed: false,
        taskCount: 0,
        taskGroups: [],
      },
    })

    expect(wrapper.text()).toContain('Queue is empty.')
    expect(wrapper.findAll('[data-testid="task-group"]')).toHaveLength(0)
  })
})
