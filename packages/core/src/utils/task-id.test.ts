import { describe, expect, it } from 'bun:test'

import { buildUniqueTaskId, formatTaskTimestamp } from './task-id'

describe('task id helpers', () => {
  it('formats timestamps in UTC-friendly filename format', () => {
    const formatted = formatTaskTimestamp(new Date('2026-03-13T15:30:12.000Z'))
    expect(formatted).toBe('20260313-153012')
  })

  it('appends a suffix when a filename collides', async () => {
    const existingIds = new Set([
      '20260313-153012-fix-a-bug-in-module-a',
      '20260313-153012-fix-a-bug-in-module-a-2',
    ])

    const nextId = await buildUniqueTaskId({
      date: new Date('2026-03-13T15:30:12.000Z'),
      description: 'Fix a bug in module A',
      exists: async (candidateId) => existingIds.has(candidateId),
    })

    expect(nextId).toBe('20260313-153012-fix-a-bug-in-module-a-3')
  })
})
