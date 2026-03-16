import slugify from 'slugify'

import { TASK_SLUG_MAX_LENGTH } from '@track/shared'

function formatTaskTimestampPart(value: number): string {
  return value.toString().padStart(2, '0')
}

export function formatTaskTimestamp(date: Date): string {
  const year = date.getUTCFullYear()
  const month = formatTaskTimestampPart(date.getUTCMonth() + 1)
  const day = formatTaskTimestampPart(date.getUTCDate())
  const hours = formatTaskTimestampPart(date.getUTCHours())
  const minutes = formatTaskTimestampPart(date.getUTCMinutes())
  const seconds = formatTaskTimestampPart(date.getUTCSeconds())

  return `${year}${month}${day}-${hours}${minutes}${seconds}`
}

export function buildTaskSlug(description: string): string {
  const slug = slugify(description, {
    lower: true,
    strict: true,
    trim: true,
  }).slice(0, TASK_SLUG_MAX_LENGTH)

  return slug.length > 0 ? slug : 'task'
}

export async function buildUniqueTaskId(options: {
  date?: Date
  description: string
  exists: (candidateId: string) => Promise<boolean>
}): Promise<string> {
  const timestamp = formatTaskTimestamp(options.date ?? new Date())
  const baseId = `${timestamp}-${buildTaskSlug(options.description)}`

  if (!(await options.exists(baseId))) {
    return baseId
  }

  let suffix = 2
  while (await options.exists(`${baseId}-${suffix}`)) {
    suffix += 1
  }

  return `${baseId}-${suffix}`
}
