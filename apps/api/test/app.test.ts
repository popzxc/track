import { describe, expect, it } from 'bun:test'
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { ConfigService, FileTaskRepository, ProjectService, TaskService } from '@track/core'

import { createApp } from '../src/app'

describe('API smoke tests', () => {
  it('lists tasks with backend sorting applied', async () => {
    const tempDirectory = await mkdtemp(join(tmpdir(), 'track-api-'))
    const staticRoot = join(tempDirectory, 'static')
    await mkdir(staticRoot, { recursive: true })
    await writeFile(join(staticRoot, 'index.html'), '<html><body>track</body></html>', 'utf8')

    const repository = new FileTaskRepository({ dataDir: join(tempDirectory, 'issues') })
    await repository.createTask({
      project: 'project-a',
      priority: 'medium',
      description: 'Middle priority task',
      source: 'cli',
    })
    await repository.createTask({
      project: 'project-a',
      priority: 'high',
      description: 'Top priority task',
      source: 'cli',
    })

    const app = createApp({
      configService: new ConfigService({ configPath: join(tempDirectory, 'missing-config.json') }),
      projectService: new ProjectService(),
      taskService: new TaskService(repository),
      staticRoot,
    })

    const response = await app.request('/api/tasks')
    const body = await response.json()

    expect(response.status).toBe(200)
    expect(body.tasks[0].priority).toBe('high')
  })

  it('patches and deletes tasks through the HTTP API', async () => {
    const tempDirectory = await mkdtemp(join(tmpdir(), 'track-api-'))
    const staticRoot = join(tempDirectory, 'static')
    const configPath = join(tempDirectory, 'config.json')
    const projectsRoot = join(tempDirectory, 'workspace')

    await mkdir(staticRoot, { recursive: true })
    await writeFile(join(staticRoot, 'index.html'), '<html><body>track</body></html>', 'utf8')
    await mkdir(join(projectsRoot, 'project-a', '.git'), { recursive: true })
    await writeFile(
      configPath,
      JSON.stringify({
        projectRoots: [projectsRoot],
        projectAliases: {},
      }),
      'utf8',
    )

    const repository = new FileTaskRepository({ dataDir: join(tempDirectory, 'issues') })
    const created = await repository.createTask({
      project: 'project-a',
      priority: 'medium',
      description: 'Update the onboarding guide',
      source: 'web',
    })

    const app = createApp({
      configService: new ConfigService({ configPath }),
      projectService: new ProjectService(),
      taskService: new TaskService(repository),
      staticRoot,
    })

    const patchResponse = await app.request(`/api/tasks/${created.task.id}`, {
      method: 'PATCH',
      headers: {
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        description: 'Update the onboarding guide for Linux users',
        priority: 'high',
        status: 'closed',
      }),
    })

    const patchedTask = await patchResponse.json()
    expect(patchedTask.status).toBe('closed')
    expect(patchedTask.priority).toBe('high')

    const deleteResponse = await app.request(`/api/tasks/${created.task.id}`, {
      method: 'DELETE',
    })

    expect(deleteResponse.status).toBe(200)
    expect(await deleteResponse.json()).toEqual({ ok: true })
  })
})
