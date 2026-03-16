import { describe, expect, it } from 'bun:test'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { FileTaskRepository } from './file-task-repository'

describe('FileTaskRepository', () => {
  it('creates a task with frontmatter and markdown body', async () => {
    const dataDirectory = await mkdtemp(join(tmpdir(), 'track-tasks-'))
    const repository = new FileTaskRepository({ dataDir: dataDirectory })

    const created = await repository.createTask({
      project: 'project-a',
      priority: 'high',
      description: 'Fix a bug in module A',
      source: 'cli',
    })

    const fileContents = await readFile(created.filePath, 'utf8')
    expect(fileContents).toContain('project: project-a')
    expect(fileContents).toContain('Fix a bug in module A')
  })

  it('moves a task between open and closed folders and updates the body', async () => {
    const dataDirectory = await mkdtemp(join(tmpdir(), 'track-tasks-'))
    const repository = new FileTaskRepository({ dataDir: dataDirectory })

    const created = await repository.createTask({
      project: 'project-a',
      priority: 'medium',
      description: 'Investigate the startup crash',
      source: 'cli',
    })

    const closedTask = await repository.updateTask(created.task.id, {
      status: 'closed',
    })
    expect(closedTask.status).toBe('closed')

    const reopenedTask = await repository.updateTask(created.task.id, {
      status: 'open',
      priority: 'high',
      description: 'Investigate the startup crash in release mode',
    })
    expect(reopenedTask.status).toBe('open')
    expect(reopenedTask.priority).toBe('high')
    expect(reopenedTask.description).toContain('release mode')
  })

  it('treats the Markdown body as the editable description source', async () => {
    const dataDirectory = await mkdtemp(join(tmpdir(), 'track-tasks-'))
    const repository = new FileTaskRepository({ dataDir: dataDirectory })

    const created = await repository.createTask({
      project: 'project-a',
      priority: 'medium',
      description: 'Original frontmatter description',
      source: 'cli',
    })

    const manuallyEditedFile = (await readFile(created.filePath, 'utf8')).replace(
      /Original frontmatter description\s*$/,
      'Edited only in the Markdown body',
    )
    await writeFile(created.filePath, manuallyEditedFile, 'utf8')

    const listedTasks = await repository.listTasks({
      includeClosed: true,
    })

    expect(listedTasks[0]?.description).toBe('Edited only in the Markdown body')
  })

  it('skips malformed files instead of breaking the whole task list', async () => {
    const dataDirectory = await mkdtemp(join(tmpdir(), 'track-tasks-'))
    const repository = new FileTaskRepository({ dataDir: dataDirectory })

    await repository.createTask({
      project: 'project-a',
      priority: 'high',
      description: 'Healthy task',
      source: 'cli',
    })

    const brokenTaskPath = join(
      dataDirectory,
      'project-a',
      'open',
      'broken-task.md',
    )
    await writeFile(
      brokenTaskPath,
      ['---', 'project: project-a', 'status: open', '---', 'This file is missing required metadata.'].join('\n'),
      'utf8',
    )

    const listedTasks = await repository.listTasks({
      includeClosed: true,
    })

    expect(listedTasks).toHaveLength(1)
    expect(listedTasks[0]?.description).toBe('Healthy task')
  })

  it('removes a task permanently', async () => {
    const dataDirectory = await mkdtemp(join(tmpdir(), 'track-tasks-'))
    const repository = new FileTaskRepository({ dataDir: dataDirectory })

    const created = await repository.createTask({
      project: 'project-a',
      priority: 'low',
      description: 'Clean up a note',
      source: 'web',
    })

    await repository.deleteTask(created.task.id)

    await expect(repository.deleteTask(created.task.id)).rejects.toThrow('was not found')
  })
})
