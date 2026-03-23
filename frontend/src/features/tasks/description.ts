import type { Task } from '../../api/types'

export interface ParsedTaskDescription {
  title: string
  summaryMarkdown?: string
  originalNote?: string
}

// =============================================================================
// Markdown Task Parsing
// =============================================================================
//
// Task files are intentionally human-editable Markdown, but the UI should not
// force users to read them as raw source. These helpers interpret the small
// heading contract that the backend writes so the drawer can present a compact
// summary and keep the raw note available only when it adds value.
export function parseTaskDescription(description: string): ParsedTaskDescription {
  const normalized = description.trim()
  const summaryMarkdown = extractMarkdownSection(normalized, 'Summary')
  const originalNoteSection = extractMarkdownSection(normalized, 'Original note')
  const normalizedWithoutFollowUps = stripMarkdownSection(normalized, 'Follow-up requests')
  const firstNonEmptyLine = normalized
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0)

  return {
    title: firstNonEmptyLine ?? normalized,
    summaryMarkdown: summaryMarkdown ?? (normalizedWithoutFollowUps || undefined),
    originalNote: originalNoteSection ? unquoteBlockquote(originalNoteSection) : undefined,
  }
}

export function extractMarkdownSection(description: string, heading: string): string | undefined {
  const marker = `## ${heading}`
  const start = description.indexOf(marker)
  if (start === -1) {
    return undefined
  }

  const afterHeading = description
    .slice(start + marker.length)
    .replace(/^[ \t\r\n]+/, '')

  if (afterHeading.length === 0) {
    return undefined
  }

  const nextHeadingIndex = afterHeading.search(/\n##\s+/)
  const rawSection = nextHeadingIndex === -1 ? afterHeading : afterHeading.slice(0, nextHeadingIndex)
  const section = rawSection.trim()
  return section.length > 0 ? section : undefined
}

export function stripMarkdownSection(description: string, heading: string): string {
  const marker = `\n## ${heading}`
  const index = description.indexOf(marker)
  return index === -1 ? description : description.slice(0, index).trimEnd()
}

export function unquoteBlockquote(value: string): string {
  return value
    .split(/\r?\n/)
    .map((line) => {
      const trimmed = line.trimStart()
      return trimmed.startsWith('>') ? trimmed.slice(1).trimStart() : line
    })
    .join('\n')
    .trim()
}

export function taskTitle(task: Task): string {
  const firstNonEmptyLine = task.description
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0)

  return firstNonEmptyLine ?? task.description.trim()
}
