import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import App from './App.vue'
import {
  buildDispatch,
  buildProject,
  buildRemoteAgentSettings,
  buildRunRecord,
  buildTask,
} from './testing/factories'

interface MockJsonRoute {
  method?: string
  path: string
  status?: number
  body: unknown
}

function installFetchRoutes(routes: MockJsonRoute[]) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const requestUrl =
      typeof input === 'string'
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url
    const resolvedUrl = new URL(requestUrl, 'http://localhost')
    const method = (init?.method ?? 'GET').toUpperCase()
    const requestPath = `${resolvedUrl.pathname}${resolvedUrl.search}`
    const route = routes.find((candidate) => (candidate.method ?? 'GET').toUpperCase() === method && candidate.path === requestPath)
      ?? routes.find((candidate) => (candidate.method ?? 'GET').toUpperCase() === method && candidate.path === resolvedUrl.pathname)

    if (!route) {
      throw new Error(`Unexpected fetch request: ${method} ${resolvedUrl.pathname}${resolvedUrl.search}`)
    }

    return new Response(JSON.stringify(route.body), {
      status: route.status ?? 200,
      headers: {
        'content-type': 'application/json',
      },
    })
  })

  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

async function mountApp() {
  const wrapper = mount(App)
  await flushPromises()
  await flushPromises()
  return wrapper
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('App shell', () => {
  it('groups tasks by project and opens the task drawer from the queue', async () => {
    const projectATask = buildTask({
      id: 'project-a/open/20260323-120000-fix-queue-layout.md',
      project: 'project-a',
      description: 'Fix queue layout\n\n## Summary\nKeep project sections visible.',
    })
    const projectBTask = buildTask({
      id: 'project-b/open/20260323-120100-review-run-history.md',
      project: 'project-b',
      description: 'Review run history\n\n## Summary\nMake latest runs easy to spot.',
    })

    installFetchRoutes([
      {
        path: '/api/projects',
        body: {
          projects: [
            buildProject({ canonicalName: 'project-a' }),
            buildProject({ canonicalName: 'project-b' }),
          ],
        },
      },
      {
        path: '/api/tasks',
        body: {
          tasks: [projectBTask, projectATask],
        },
      },
      {
        path: '/api/runs?limit=200',
        body: {
          runs: [
            buildRunRecord(
              { ...projectATask },
              {
                dispatchId: 'dispatch-queue',
                taskId: projectATask.id,
                project: projectATask.project,
              },
            ),
          ],
        },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings(),
      },
    ])

    const wrapper = await mountApp()

    const groups = wrapper.findAll('[data-testid="task-group"]')
    expect(groups.map((group) => group.attributes('data-project'))).toEqual(['project-a', 'project-b'])

    await wrapper.get(`[data-task-id="${projectATask.id}"]`).trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="task-drawer"]').text()).toContain('Fix queue layout')
    expect(wrapper.get('[data-testid="run-latest-badge"]').text()).toBe('Latest')
  })

  it('surfaces dispatch failures as a user-visible error banner', async () => {
    const task = buildTask()

    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [task] },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings(),
      },
      {
        method: 'POST',
        path: `/api/tasks/${task.id}/dispatch`,
        status: 502,
        body: {
          error: {
            code: 'REMOTE_DISPATCH_FAILED',
            message: 'Runner offline.',
          },
        },
      },
    ])

    const wrapper = await mountApp()

    await wrapper.get(`[data-task-id="${task.id}"]`).trigger('click')
    await flushPromises()
    await wrapper.get('[data-testid="drawer-primary-action"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="error-banner"]').text()).toContain('Runner offline.')
  })

  it('updates the visible run state immediately after a successful dispatch response', async () => {
    const task = buildTask()

    installFetchRoutes([
      {
        path: '/api/projects',
        body: { projects: [buildProject()] },
      },
      {
        path: '/api/tasks',
        body: { tasks: [task] },
      },
      {
        path: '/api/runs?limit=200',
        body: { runs: [] },
      },
      {
        path: '/api/events/version',
        body: { version: 1 },
      },
      {
        path: '/api/remote-agent',
        body: buildRemoteAgentSettings(),
      },
      {
        method: 'POST',
        path: `/api/tasks/${task.id}/dispatch`,
        body: buildDispatch({
          dispatchId: 'dispatch-started',
          taskId: task.id,
          project: task.project,
          status: 'running',
          summary: 'The remote agent is working in the prepared environment.',
        }),
      },
    ])

    const wrapper = await mountApp()

    await wrapper.get(`[data-task-id="${task.id}"]`).trigger('click')
    await flushPromises()
    await wrapper.get('[data-testid="drawer-primary-action"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="run-history-item"]').text()).toContain('Agent running')
  })
})
