import type {
  ProjectInfo,
  RemoteAgentSettings,
  ReviewRecord,
  ReviewRunRecord,
  ReviewSummary,
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

export function buildReview(overrides: Partial<ReviewRecord> = {}): ReviewRecord {
  return {
    id: '20260326-120000-review-pr-42',
    pullRequestUrl: 'https://github.com/acme/project-a/pull/42',
    pullRequestNumber: 42,
    pullRequestTitle: 'Fix queue layout',
    repositoryFullName: 'acme/project-a',
    repoUrl: 'https://github.com/acme/project-a',
    gitUrl: 'git@github.com:acme/project-a.git',
    baseBranch: 'main',
    workspaceKey: 'project-a',
    project: 'project-a',
    mainUser: 'octocat',
    defaultReviewPrompt: 'Focus on regressions and missing tests.',
    extraInstructions: 'Pay extra attention to the queue layout changes.',
    createdAt: '2026-03-26T12:00:00.000Z',
    updatedAt: '2026-03-26T12:00:00.000Z',
    ...overrides,
  }
}

export function buildReviewRun(overrides: Partial<ReviewRunRecord> = {}): ReviewRunRecord {
  return {
    dispatchId: 'review-dispatch-123',
    reviewId: '20260326-120000-review-pr-42',
    pullRequestUrl: 'https://github.com/acme/project-a/pull/42',
    repositoryFullName: 'acme/project-a',
    workspaceKey: 'project-a',
    status: 'succeeded',
    createdAt: '2026-03-26T12:05:00.000Z',
    updatedAt: '2026-03-26T12:06:00.000Z',
    finishedAt: '2026-03-26T12:06:00.000Z',
    remoteHost: '127.0.0.1',
    branchName: 'track-review/review-dispatch-123',
    worktreePath: '/tmp/review-worktree',
    followUpRequest: undefined,
    targetHeadOid: 'abc123def456',
    summary: 'Submitted a GitHub review with two inline comments.',
    reviewSubmitted: true,
    githubReviewId: '1001',
    githubReviewUrl: 'https://github.com/acme/project-a/pull/42#pullrequestreview-1001',
    notes: 'Generated from the frontend test fixture.',
    ...overrides,
  }
}

export function buildReviewSummary(overrides: {
  review?: Partial<ReviewRecord>
  latestRun?: Partial<ReviewRunRecord> | null
} = {}): ReviewSummary {
  const review = buildReview(overrides.review)

  return {
    review,
    latestRun: overrides.latestRun === null
      ? undefined
      : buildReviewRun({
        reviewId: review.id,
        pullRequestUrl: review.pullRequestUrl,
        repositoryFullName: review.repositoryFullName,
        workspaceKey: review.workspaceKey,
        ...overrides.latestRun,
      }),
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
    reviewFollowUp: {
      enabled: false,
      mainUser: undefined,
      defaultReviewPrompt: undefined,
    },
    ...overrides,
  }
}
