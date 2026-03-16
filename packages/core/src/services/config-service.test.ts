import { describe, expect, it } from 'bun:test'
import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { ConfigService } from './config-service'

describe('ConfigService', () => {
  it('normalizes llama.cpp configuration when local models are enabled', async () => {
    const tempDirectory = await mkdtemp(join(tmpdir(), 'track-config-'))
    const configPath = join(tempDirectory, 'config.json')

    await writeFile(
      configPath,
      JSON.stringify({
        projectRoots: ['/tmp/work'],
        projectAliases: {
          'proj-a': 'project-a',
        },
        ai: {
          provider: 'llama-cpp',
          llamaCpp: {
            modelPath: '/models/task-parser.gguf',
            llamaCliPath: '/opt/llama.cpp/bin/llama-cli',
          },
        },
      }),
      'utf8',
    )

    const configService = new ConfigService({ configPath })
    const config = await configService.loadConfig()

    expect(config.ai.provider).toBe('llama-cpp')
    if (config.ai.provider !== 'llama-cpp') {
      throw new Error('Expected llama.cpp configuration.')
    }

    expect(config.ai.llamaCpp.modelPath).toBe('/models/task-parser.gguf')
    expect(config.ai.llamaCpp.llamaCliPath).toBe('/opt/llama.cpp/bin/llama-cli')
  })
})
