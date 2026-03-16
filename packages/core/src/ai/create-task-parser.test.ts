import { describe, expect, it } from 'bun:test'

import { createTaskParser } from './create-task-parser'
import { LlamaCppTaskParser } from './llama-cpp-task-parser'

describe('createTaskParser', () => {
  it('selects the llama.cpp parser when configured', () => {
    const parser = createTaskParser({
      projectRoots: ['/tmp/work'],
      projectAliases: {},
      ai: {
        provider: 'llama-cpp',
        llamaCpp: {
          modelPath: '/models/task-parser.gguf',
        },
      },
    })

    expect(parser).toBeInstanceOf(LlamaCppTaskParser)
  })
})
