import { spawn } from 'node:child_process'

import { DEFAULT_LLAMA_CPP_BINARY, parsedTaskCandidateSchema } from '@track/shared'

import { TrackError } from '../errors'
import type { AiTaskParser } from './provider'
import { buildLlamaCppPrompt } from './task-parser-prompt'

// =============================================================================
// llama.cpp Adapter
// =============================================================================
//
// This adapter exists because the local-model path speaks in terms of a CLI
// process, not an SDK. We keep the process boundary explicit so tests can stub
// it cleanly and the rest of the app still sees the same parser interface.
//
export interface LlamaCppCommandResult {
  exitCode: number | null
  stderr: string
  stdout: string
}

export type LlamaCppCommandRunner = (input: {
  args: string[]
  binaryPath: string
}) => Promise<LlamaCppCommandResult>

async function runLlamaCppCommand(input: {
  args: string[]
  binaryPath: string
}): Promise<LlamaCppCommandResult> {
  // The command runner is separated from the parser so tests can validate the
  // parsing logic without needing a real local model binary on disk.
  return new Promise((resolve, reject) => {
    const childProcess = spawn(input.binaryPath, input.args, {
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    let stdout = ''
    let stderr = ''

    childProcess.stdout.on('data', (chunk) => {
      stdout += chunk.toString()
    })

    childProcess.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })

    childProcess.on('error', reject)
    childProcess.on('close', (exitCode) => {
      resolve({
        exitCode,
        stderr,
        stdout,
      })
    })
  })
}

function extractJsonFromModelOutput(output: string): string {
  // Local models sometimes wrap JSON in extra narration or code fences, so we
  // salvage the structured payload when it is still recoverable.
  const trimmedOutput = output.trim()
  if (trimmedOutput.length === 0) {
    throw new TrackError('AI_PARSE_FAILED', 'AI parse failure. The local model returned an empty response.')
  }

  const withoutCodeFences = trimmedOutput
    .replace(/^```json\s*/i, '')
    .replace(/^```\s*/i, '')
    .replace(/\s*```$/i, '')
    .trim()

  if (withoutCodeFences.startsWith('{') && withoutCodeFences.endsWith('}')) {
    return withoutCodeFences
  }

  const firstBraceIndex = withoutCodeFences.indexOf('{')
  const lastBraceIndex = withoutCodeFences.lastIndexOf('}')
  if (firstBraceIndex >= 0 && lastBraceIndex > firstBraceIndex) {
    return withoutCodeFences.slice(firstBraceIndex, lastBraceIndex + 1)
  }

  throw new TrackError('AI_PARSE_FAILED', 'AI parse failure. The local model did not return valid JSON.')
}

export class LlamaCppTaskParser implements AiTaskParser {
  private readonly binaryPath: string
  private readonly modelPath: string
  private readonly runCommand: LlamaCppCommandRunner

  constructor(options: {
    binaryPath?: string
    modelPath: string
    runCommand?: LlamaCppCommandRunner
  }) {
    this.binaryPath = options.binaryPath ?? DEFAULT_LLAMA_CPP_BINARY
    this.modelPath = options.modelPath
    this.runCommand = options.runCommand ?? runLlamaCppCommand
  }

  async parseTask(input: {
    rawText: string
    allowedProjects: Array<{
      canonicalName: string
      aliases: string[]
    }>
  }) {
    const requestTimestamp = new Date().toISOString()
    const prompt = buildLlamaCppPrompt(input)

    // We choose deterministic sampling here because this path is a parser, not
    // a creative assistant. Predictability matters more than expressiveness.
    // TODO: Surface additional llama.cpp inference knobs only if real local-model tuning needs emerge.
    const args = [
      '-m',
      this.modelPath,
      '-p',
      prompt,
      '-n',
      '256',
      '--temp',
      '0',
    ]

    try {
      const result = await this.runCommand({
        binaryPath: this.binaryPath,
        args,
      })

      // A CLI exit failure is different from a schema failure, but both should
      // collapse into the same user-facing "parse failed" outcome.
      if (result.exitCode !== 0) {
        throw new TrackError(
          'AI_PARSE_FAILED',
          `AI parse failure. llama-cli exited with code ${result.exitCode ?? 'unknown'}.`,
        )
      }

      const parsedCandidate = parsedTaskCandidateSchema.parse(
        JSON.parse(extractJsonFromModelOutput(result.stdout)),
      )

      if (process.env.TRACK_DEBUG_AI === '1') {
        console.error(
          JSON.stringify({
            timestamp: requestTimestamp,
            event: 'ai_parse',
            provider: 'llama-cpp',
            binaryPath: this.binaryPath,
            modelPath: this.modelPath,
            ok: true,
          }),
        )
      }

      return parsedCandidate
    } catch (error) {
      console.error(
        JSON.stringify({
          timestamp: requestTimestamp,
          event: 'ai_parse',
          provider: 'llama-cpp',
          binaryPath: this.binaryPath,
          modelPath: this.modelPath,
          ok: false,
          error: error instanceof Error ? error.message : 'Unknown error',
        }),
      )

      if (error instanceof TrackError) {
        throw error
      }

      throw new TrackError(
        'AI_PARSE_FAILED',
        'AI parse failure. Please try again with a more explicit task description.',
        { cause: error },
      )
    }
  }
}
