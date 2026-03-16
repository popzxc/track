export const TASK_PARSER_SYSTEM_PROMPT = [
  'You convert short CLI issue notes into structured task data.',
  'Return only fields supported by the schema.',
  'Prefer concise, actionable descriptions.',
].join(' ')

export const TASK_PARSER_DEVELOPER_PROMPT = [
  'Choose priority from high, medium, or low.',
  'Default priority to medium when the text does not clearly set one.',
  'Choose project only from the provided canonical project names.',
  'Use aliases only for matching and always output the canonical project name.',
  'If the project is ambiguous or missing, output project as null and confidence as low.',
  'If you are uncertain about project selection, set confidence to low.',
  'Respond with strict JSON that matches the provided schema.',
].join(' ')

export function buildTaskParserPayload(input: {
  rawText: string
  allowedProjects: Array<{
    canonicalName: string
    aliases: string[]
  }>
}) {
  return {
    rawText: input.rawText,
    allowedProjects: input.allowedProjects,
    expectedJsonShape: {
      project: 'canonical-project-name-or-null',
      priority: 'high|medium|low',
      description: 'Concise actionable sentence',
      confidence: 'high|low',
      reason: 'Optional short explanation',
    },
  }
}

export function buildLlamaCppPrompt(input: {
  rawText: string
  allowedProjects: Array<{
    canonicalName: string
    aliases: string[]
  }>
}) {
  return [
    TASK_PARSER_SYSTEM_PROMPT,
    TASK_PARSER_DEVELOPER_PROMPT,
    'Return only JSON. Do not use Markdown fences.',
    JSON.stringify(buildTaskParserPayload(input), null, 2),
  ].join('\n\n')
}
