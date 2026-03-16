import { describe, expect, it } from 'bun:test'
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import type { ParsedTaskCandidate } from '@track/shared'

import type { AiTaskParser } from '../ai/provider'
import { ConfigService } from './config-service'
import { ProjectService } from './project-service'
import { TaskCaptureService, validateParsedTaskCandidate } from './task-capture-service'
import { FileTaskRepository } from '../storage/file-task-repository'

class StaticAiTaskParser implements AiTaskParser {
  constructor(private readonly response: ParsedTaskCandidate) {}

  async parseTask() {
    return this.response
  }
}

describe('TaskCaptureService', () => {
  it('normalizes a valid parsed candidate', () => {
    const normalized = validateParsedTaskCandidate(
      {
        project: 'project-a',
        priority: 'high',
        description: 'Fix the flaky test',
        confidence: 'high',
      },
      [
        {
          canonicalName: 'project-a',
          path: '/tmp/project-a',
          aliases: ['proj-a'],
        },
      ],
    )

    expect(normalized.project).toBe('project-a')
  })

  it('rejects ambiguous project selections', () => {
    expect(() =>
      validateParsedTaskCandidate(
        {
          project: null,
          priority: 'medium',
          description: 'Investigate the failure',
          confidence: 'low',
        },
        [
          {
            canonicalName: 'project-a',
            path: '/tmp/project-a',
            aliases: ['proj-a'],
          },
        ],
      ),
    ).toThrow('Could not determine a valid project')
  })

  it('creates a task file from CLI text using the shared services', async () => {
    const tempDirectory = await mkdtemp(join(tmpdir(), 'track-capture-'))
    const configPath = join(tempDirectory, 'config.json')
    const projectsRoot = join(tempDirectory, 'workspace')
    const dataDirectory = join(tempDirectory, 'issues')

    await mkdir(join(projectsRoot, 'project-a', '.git'), { recursive: true })
    await writeFile(
      configPath,
      JSON.stringify({
        projectRoots: [projectsRoot],
        projectAliases: {
          'proj-a': 'project-a',
        },
      }),
      'utf8',
    )

    const captureService = new TaskCaptureService({
      aiTaskParser: new StaticAiTaskParser({
        project: 'project-a',
        priority: 'high',
        description: 'Fix the flaky test',
        confidence: 'high',
      }),
      configService: new ConfigService({ configPath }),
      projectService: new ProjectService(),
      taskRepository: new FileTaskRepository({ dataDir: dataDirectory }),
    })

    const createdTask = await captureService.createTaskFromText({
      rawText: 'proj-a prio high fix the flaky test',
      source: 'cli',
    })

    expect(createdTask.task.project).toBe('project-a')
    expect(createdTask.task.status).toBe('open')
  })
})
