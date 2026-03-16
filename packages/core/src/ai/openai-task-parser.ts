import OpenAI from 'openai'

import { OPENAI_TASK_MODEL, parsedTaskCandidateSchema } from '@track/shared'

import { TrackError } from '../errors'
import type { AiTaskParser } from './provider'
import { buildTaskParserPayload, TASK_PARSER_DEVELOPER_PROMPT, TASK_PARSER_SYSTEM_PROMPT } from './task-parser-prompt'

const RESPONSE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    project: {
      anyOf: [
        { type: 'string', minLength: 1 },
        { type: 'null' },
      ],
    },
    priority: {
      type: 'string',
      enum: ['high', 'medium', 'low'],
    },
    description: {
      type: 'string',
      minLength: 1,
    },
    confidence: {
      type: 'string',
      enum: ['high', 'low'],
    },
    reason: {
      type: 'string',
    },
  },
  required: ['project', 'priority', 'description', 'confidence'],
} as const

export class OpenAiTaskParser implements AiTaskParser {
  private readonly client: OpenAI
  private readonly model: string

  constructor(options?: { apiKey?: string; model?: string; client?: OpenAI }) {
    const apiKey = options?.apiKey ?? process.env.OPENAI_API_KEY
    if (!options?.client && !apiKey) {
      throw new TrackError('OPENAI_API_KEY_MISSING', 'OPENAI_API_KEY is not set.')
    }

    this.client = options?.client ?? new OpenAI({ apiKey })
    this.model = options?.model ?? OPENAI_TASK_MODEL
  }

  async parseTask(input: {
    rawText: string
    allowedProjects: Array<{
      canonicalName: string
      aliases: string[]
    }>
  }) {
    const requestTimestamp = new Date().toISOString()

    try {
      const response = await this.client.responses.create({
        model: this.model,
        input: [
          {
            role: 'system',
            content: [
              {
                type: 'input_text',
                text: TASK_PARSER_SYSTEM_PROMPT,
              },
            ],
          },
          {
            role: 'developer',
            content: [
              {
                type: 'input_text',
                text: TASK_PARSER_DEVELOPER_PROMPT,
              },
            ],
          },
          {
            role: 'user',
            content: [
              {
                type: 'input_text',
                text: JSON.stringify(buildTaskParserPayload(input), null, 2),
              },
            ],
          },
        ],
        text: {
          format: {
            type: 'json_schema',
            name: 'parsed_task_candidate',
            strict: true,
            schema: RESPONSE_SCHEMA,
          },
        },
      })

      const parsedJson = JSON.parse(response.output_text)
      const parsedCandidate = parsedTaskCandidateSchema.parse(parsedJson)

      if (process.env.TRACK_DEBUG_AI === '1') {
        console.error(
          JSON.stringify({
            timestamp: requestTimestamp,
            event: 'ai_parse',
            model: this.model,
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
          model: this.model,
          ok: false,
          error: error instanceof Error ? error.message : 'Unknown error',
        }),
      )

      throw new TrackError(
        'AI_PARSE_FAILED',
        'AI parse failure. Please try again with a more explicit task description.',
        { cause: error },
      )
    }
  }
}
