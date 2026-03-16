import type { TrackConfig } from '@track/shared'
import { LlamaCppTaskParser, type LlamaCppCommandRunner } from './llama-cpp-task-parser'
import { OpenAiTaskParser as OpenAiTaskParserImplementation } from './openai-task-parser'
import type OpenAI from 'openai'

// =============================================================================
// Provider Selection
// =============================================================================
//
// The rest of the app depends on a narrow parser interface. This factory is the
// only place that knows how config selects a concrete provider implementation.
//
export function createTaskParser(
  config: TrackConfig,
  options?: {
    llamaCppRunner?: LlamaCppCommandRunner
    openAiClient?: OpenAI
  },
) {
  // This branching stays intentionally small so swapping providers later does
  // not ripple through the CLI capture flow or the repository logic.
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
