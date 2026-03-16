import type { ParsedTaskCandidate } from '@track/shared'

export interface AiTaskParser {
  parseTask(input: {
    rawText: string
    allowedProjects: Array<{
      canonicalName: string
      aliases: string[]
    }>
  }): Promise<ParsedTaskCandidate>
}
