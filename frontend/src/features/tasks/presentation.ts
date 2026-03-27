import type {
  ProjectInfo,
  RemoteAgentSettings,
  ReviewRunRecord,
  RunRecord,
  Task,
  TaskDispatch,
} from '../../api/types'

export const RECENT_FAILURE_WINDOW_MS = 15 * 60 * 1000

export interface TaskGroup {
  project: string
  tasks: Task[]
}

export type DrawerPrimaryAction = 'reopen' | 'cancel' | 'continue' | 'start'
export type DispatchPresentationKind = 'task' | 'review'

interface DispatchPresentationRecord {
  status: TaskDispatch['status']
  updatedAt: string
  finishedAt?: string
  summary?: string
  notes?: string
  errorMessage?: string
}

// =============================================================================
// Queue And Run Presentation
// =============================================================================
//
// The frontend now renders tasks in grouped sections and derives most UI state
// from the latest dispatch record. Keeping these helpers pure gives us a stable
// place to test behavior without coupling assertions to the exact Vue layout.
export function groupTasksByProject(tasks: Task[]): TaskGroup[] {
  const grouped = new Map<string, Task[]>()

  for (const task of tasks) {
    const existingGroup = grouped.get(task.project)
    if (existingGroup) {
      existingGroup.push(task)
      continue
    }

    grouped.set(task.project, [task])
  }

  return Array.from(grouped.entries())
    .sort(([leftProject], [rightProject]) => leftProject.localeCompare(rightProject))
    .map(([project, groupedTasks]) => ({ project, tasks: groupedTasks }))
}

export function latestDispatchByTaskId(runs: RunRecord[]): Record<string, TaskDispatch> {
  const latestByTaskId: Record<string, TaskDispatch> = {}

  for (const run of runs) {
    const existing = latestByTaskId[run.task.id]
    if (!existing || Date.parse(run.dispatch.createdAt) > Date.parse(existing.createdAt)) {
      latestByTaskId[run.task.id] = run.dispatch
    }
  }

  return latestByTaskId
}

export function taskReference(task: Task): string {
  return (task.id.split('/').pop() ?? task.id).replace(/\.md$/i, '')
}

export function formatDateTime(value?: string): string {
  if (!value) {
    return 'Unknown'
  }

  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }

  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(date)
}

export function formatTaskTimestamp(task: Task): string {
  return task.updatedAt === task.createdAt
    ? `Created ${formatDateTime(task.createdAt)}`
    : `Updated ${formatDateTime(task.updatedAt)}`
}

export function isRecentFailure(dispatch: DispatchPresentationRecord, now = Date.now()): boolean {
  if (dispatch.status !== 'failed') {
    return false
  }

  const timestamp = dispatch.finishedAt ?? dispatch.updatedAt
  const finishedAt = Date.parse(timestamp)
  if (Number.isNaN(finishedAt)) {
    return false
  }

  return now - finishedAt <= RECENT_FAILURE_WINDOW_MS
}

export function dispatchStatusLabel(
  dispatch?: DispatchPresentationRecord | null,
  now = Date.now(),
): string {
  if (!dispatch) {
    return 'No run'
  }

  switch (dispatch.status) {
    case 'preparing':
      return 'Preparing environment'
    case 'running':
      return 'Agent running'
    case 'succeeded':
      return 'Succeeded'
    case 'blocked':
      return 'Blocked'
    case 'canceled':
      return 'Canceled'
    case 'failed':
      if (!isRecentFailure(dispatch, now) && dispatch.finishedAt) {
        return `Failed on ${formatDateTime(dispatch.finishedAt)}`
      }

      return 'Failed'
    default:
      return 'No run'
  }
}

export function dispatchBadgeClass(
  dispatch?: DispatchPresentationRecord | null,
  now = Date.now(),
): string {
  if (!dispatch) {
    return 'border-fg2/15 bg-bg0/80 text-fg3'
  }

  switch (dispatch.status) {
    case 'preparing':
      return 'border-yellow/30 bg-yellow/10 text-yellow'
    case 'running':
      return 'border-blue/30 bg-blue/10 text-blue'
    case 'succeeded':
      return 'border-green/30 bg-green/10 text-green'
    case 'blocked':
      return 'border-orange/30 bg-orange/10 text-orange'
    case 'canceled':
      return 'border-fg2/20 bg-bg3/60 text-fg2'
    case 'failed':
      return isRecentFailure(dispatch, now)
        ? 'border-red/30 bg-red/10 text-red'
        : 'border-red/20 bg-red/5 text-red/70'
    default:
      return 'border-fg2/15 bg-bg0/80 text-fg3'
  }
}

export function priorityBadgeClass(priority: Task['priority']): string {
  switch (priority) {
    case 'high':
      return 'border-red/20 bg-red/8 text-red'
    case 'medium':
      return 'border-yellow/20 bg-yellow/8 text-yellow'
    case 'low':
      return 'border-aqua/20 bg-aqua/8 text-aqua'
  }
}

export function taskStatusBadgeClass(status: Task['status']): string {
  return status === 'open'
    ? 'border-blue/25 bg-blue/10 text-blue'
    : 'border-fg2/20 bg-bg3/60 text-fg2'
}

export function dispatchSummary(
  dispatch?: DispatchPresentationRecord | ReviewRunRecord | null,
  kind: DispatchPresentationKind = 'task',
): string {
  if (!dispatch) {
    return kind === 'review'
      ? 'No PR review run has been recorded yet.'
      : 'No agent run has been recorded for this task yet.'
  }

  switch (dispatch.status) {
    case 'preparing':
      return dispatch.summary
        ?? (kind === 'review'
          ? 'Preparing the remote checkout, review worktree, and prompt.'
          : 'Preparing the remote checkout, worktree, and prompt.')
    case 'running':
      return dispatch.summary
        ?? (kind === 'review'
          ? 'The remote agent is reviewing the prepared pull request.'
          : 'The remote agent is working in the prepared environment.')
    case 'succeeded':
      return dispatch.summary
        ?? (kind === 'review'
          ? 'The remote agent finished the PR review successfully.'
          : 'The remote agent finished successfully.')
    case 'blocked':
      return dispatch.summary
        ?? dispatch.notes
        ?? (kind === 'review'
          ? 'The review run stopped for human follow-up.'
          : 'The run stopped for human follow-up.')
    case 'canceled':
      return dispatch.summary
        ?? (kind === 'review' ? 'The remote review run was canceled.' : 'The remote run was canceled.')
    case 'failed':
      return dispatch.errorMessage
        ?? dispatch.summary
        ?? (kind === 'review'
          ? 'The remote review run failed.'
          : 'The remote run failed.')
    default:
      return kind === 'review'
        ? 'No PR review run has been recorded yet.'
        : 'No agent run has been recorded for this task yet.'
  }
}

export function drawerPrimaryAction(task: Task, dispatch?: TaskDispatch | null): DrawerPrimaryAction {
  if (task.status === 'closed') {
    return 'reopen'
  }

  if (dispatch?.status === 'preparing' || dispatch?.status === 'running') {
    return 'cancel'
  }

  if (dispatch?.branchName && dispatch.worktreePath) {
    return 'continue'
  }

  return 'start'
}

export function mergeProjects(...projectGroups: ProjectInfo[][]): ProjectInfo[] {
  const byCanonicalName = new Map<string, ProjectInfo>()

  for (const group of projectGroups) {
    for (const project of group) {
      const existing = byCanonicalName.get(project.canonicalName)
      if (!existing) {
        byCanonicalName.set(project.canonicalName, project)
        continue
      }

      byCanonicalName.set(project.canonicalName, {
        canonicalName: project.canonicalName,
        path: project.path || existing.path,
        aliases: Array.from(new Set([...existing.aliases, ...project.aliases])),
        metadata: project.metadata ?? existing.metadata,
      })
    }
  }

  return Array.from(byCanonicalName.values()).sort((left, right) =>
    left.canonicalName.localeCompare(right.canonicalName),
  )
}

export function getRunStartDisabledReason(
  task: Task,
  availableProjects: ProjectInfo[],
  remoteAgentSettings: RemoteAgentSettings | null,
): string | undefined {
  if (remoteAgentSettings && !remoteAgentSettings.configured) {
    return 'Configure the remote agent with `track` locally before dispatching tasks.'
  }

  const project = availableProjects.find((candidate) => candidate.canonicalName === task.project)
  if (!project?.metadata) {
    return 'Project details are not available yet.'
  }

  if (
    project.metadata.repoUrl.trim().length === 0 ||
    project.metadata.gitUrl.trim().length === 0 ||
    project.metadata.baseBranch.trim().length === 0
  ) {
    return 'Complete the project details before dispatching.'
  }

  return undefined
}
