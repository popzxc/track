import { collapseHomePath, getErrorMessage, isTrackError } from '@track/core'

export function formatCreatedTaskOutput(result: { filePath: string; task: { project: string; priority: string; status: string } }) {
  return [
    'Created task:',
    `  project: ${result.task.project}`,
    `  priority: ${result.task.priority}`,
    `  status: ${result.task.status}`,
    `  file: ${collapseHomePath(result.filePath)}`,
  ].join('\n')
}

export function formatCliError(error: unknown) {
  if (isTrackError(error)) {
    return error.message
  }

  return getErrorMessage(error)
}
