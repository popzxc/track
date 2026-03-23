import type {
  ProjectInfo,
  RemoteAgentSettings,
  RunRecord,
  Task,
  TaskDispatch,
} from '../api/types'

export function buildTask(overrides: Partial<Task> = {}): Task {
  return {
    id: 'project-a/open/20260323-120000-fix-queue-layout.md',
    project: 'project-a',
    priority: 'high',
    status: 'open',
    description: 'Fix queue layout\n\n## Summary\nImprove task grouping.',
    createdAt: '2026-03-23T12:00:00.000Z',
    updatedAt: '2026-03-23T12:00:00.000Z',
    source: 'cli',
    ...overrides,
  }
}

export function buildDispatch(overrides: Partial<TaskDispatch> = {}): TaskDispatch {
  return {
    dispatchId: 'dispatch-123',
    taskId: 'project-a/open/20260323-120000-fix-queue-layout.md',
    project: 'project-a',
    status: 'succeeded',
    createdAt: '2026-03-23T12:05:00.000Z',
    updatedAt: '2026-03-23T12:06:00.000Z',
    finishedAt: '2026-03-23T12:06:00.000Z',
    remoteHost: '127.0.0.1',
    branchName: 'track/dispatch-123',
    worktreePath: '/tmp/worktree',
    pullRequestUrl: 'https://github.com/acme/project-a/pull/42',
    summary: 'Completed the task successfully.',
    ...overrides,
  }
}

export function buildRunRecord(
  taskOverrides: Partial<Task> = {},
  dispatchOverrides: Partial<TaskDispatch> = {},
): RunRecord {
  const task = buildTask(taskOverrides)
  const dispatch = buildDispatch({
    taskId: task.id,
    project: task.project,
    ...dispatchOverrides,
  })

  return { task, dispatch }
}

export function buildProject(overrides: Partial<ProjectInfo> = {}): ProjectInfo {
  return {
    canonicalName: 'project-a',
    path: '/workspace/project-a',
    aliases: ['proj-a'],
    metadata: {
      repoUrl: 'https://github.com/acme/project-a',
      gitUrl: 'git@github.com:acme/project-a.git',
      baseBranch: 'main',
      description: 'Project A',
    },
    ...overrides,
  }
}

export function buildRemoteAgentSettings(
  overrides: Partial<RemoteAgentSettings> = {},
): RemoteAgentSettings {
  return {
    configured: true,
    host: '127.0.0.1',
    user: 'track',
    port: 2222,
    shellPrelude: 'export PATH="/opt/track-testing/bin:$PATH"',
    ...overrides,
  }
}
