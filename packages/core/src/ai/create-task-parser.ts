import type { TrackConfig } from '@track/shared'
import { LlamaCppTaskParser, type LlamaCppCommandRunner } from './llama-cpp-task-parser'
import { OpenAiTaskParser as OpenAiTaskParserImplementation } from './openai-task-parser'
import type OpenAI from 'openai'

export function createTaskParser(
  config: TrackConfig,
  options?: {
    llamaCppRunner?: LlamaCppCommandRunner
    openAiClient?: OpenAI
  },
) {
  if (config.ai.provider === 'llama-cpp') {
    return new LlamaCppTaskParser({
      modelPath: config.ai.llamaCpp.modelPath,
      binaryPath: config.ai.llamaCpp.llamaCliPath,
      runCommand: options?.llamaCppRunner,
    })
  }

  return new OpenAiTaskParserImplementation({
    client: options?.openAiClient,
    model: config.ai.openai.model,
  })
}
