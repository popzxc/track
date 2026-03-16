import { describe, expect, it } from 'bun:test'

import { parsedTaskCandidateSchema, taskSchema, trackConfigSchema } from './schemas'

describe('shared schemas', () => {
  it('accepts valid task payloads', () => {
    const result = taskSchema.parse({
      id: '20260313-153012-fix-a-bug-in-module-a',
      project: 'project-x-repo',
      priority: 'high',
      status: 'open',
      description: 'Fix a bug in module A',
      createdAt: '2026-03-13T15:30:12.000Z',
      updatedAt: '2026-03-13T15:30:12.000Z',
      source: 'cli',
    })

    expect(result.project).toBe('project-x-repo')
  })

  it('rejects a parser payload with an unsupported project type', () => {
    const result = parsedTaskCandidateSchema.safeParse({
      project: 42,
      priority: 'medium',
      description: 'Investigate a flaky test',
      confidence: 'high',
    })

    expect(result.success).toBe(false)
  })

  it('requires a model path when llama.cpp is selected', () => {
    const result = trackConfigSchema.safeParse({
      projectRoots: ['/tmp/work'],
      projectAliases: {},
      ai: {
        provider: 'llama-cpp',
      },
    })

    expect(result.success).toBe(false)
  })
})
