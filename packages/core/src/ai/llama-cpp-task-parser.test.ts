import { describe, expect, it } from 'bun:test'

import { LlamaCppTaskParser } from './llama-cpp-task-parser'

describe('LlamaCppTaskParser', () => {
  it('parses fenced JSON emitted by llama-cli', async () => {
    const parser = new LlamaCppTaskParser({
      modelPath: '/models/task-parser.gguf',
      binaryPath: 'llama-cli',
      runCommand: async ({ binaryPath, args }) => {
        expect(binaryPath).toBe('llama-cli')
        expect(args).toContain('/models/task-parser.gguf')

        return {
          exitCode: 0,
          stderr: '',
          stdout: [
            '```json',
            '{"project":"project-a","priority":"high","description":"Fix the flaky test","confidence":"high"}',
            '```',
          ].join('\n'),
        }
      },
    })

    const parsedTask = await parser.parseTask({
      rawText: 'proj-a prio high fix the flaky test',
      allowedProjects: [
        {
          canonicalName: 'project-a',
          aliases: ['proj-a'],
        },
      ],
    })

    expect(parsedTask.project).toBe('project-a')
    expect(parsedTask.priority).toBe('high')
  })

  it('fails clearly when llama-cli does not return JSON', async () => {
    const parser = new LlamaCppTaskParser({
      modelPath: '/models/task-parser.gguf',
      runCommand: async () => ({
        exitCode: 0,
        stderr: '',
        stdout: 'I am not sure.',
      }),
    })

    await expect(
      parser.parseTask({
        rawText: 'investigate a flaky test',
        allowedProjects: [
          {
            canonicalName: 'project-a',
            aliases: ['proj-a'],
          },
        ],
      }),
    ).rejects.toThrow('did not return valid JSON')
  })
})
