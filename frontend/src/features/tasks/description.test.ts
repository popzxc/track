import { describe, expect, it } from 'vitest'

import {
  extractMarkdownSection,
  parseTaskDescription,
} from './description'

describe('parseTaskDescription', () => {
  it('extracts summary and original note from the markdown contract', () => {
    const parsed = parseTaskDescription(`
Fix queue layout

## Summary
Keep each project in its own visible block.

## Original note
> make the task grouping more pronounced
`)

    expect(parsed.title).toBe('Fix queue layout')
    expect(parsed.summaryMarkdown).toBe('Keep each project in its own visible block.')
    expect(parsed.originalNote).toBe('make the task grouping more pronounced')
  })

  it('falls back to the main task body while ignoring appended follow-up history', () => {
    const parsed = parseTaskDescription(`
Investigate stale run state

The latest run still shows as running in the UI.

## Follow-up requests
### 2026-03-23T10:00:00.000Z

Check whether /api/runs reconciles remote state.
`)

    expect(parsed.summaryMarkdown).toContain('The latest run still shows as running in the UI.')
    expect(parsed.summaryMarkdown).not.toContain('Follow-up requests')
  })
})

describe('extractMarkdownSection', () => {
  it('returns undefined when the requested section is absent', () => {
    expect(extractMarkdownSection('Fix queue layout', 'Summary')).toBeUndefined()
  })
})
