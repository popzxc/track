export interface ApiErrorResponse {
  error: {
    code: string
    message: string
  }
}

export interface DeleteTaskResponse {
  ok: true
}

export interface DispatchesResponse {
  dispatches: TaskDispatch[]
}

export interface RunsResponse {
  runs: RunRecord[]
}

export interface ReviewsResponse {
  reviews: ReviewSummary[]
}

export interface ReviewRunsResponse {
  runs: ReviewRunRecord[]
}

export interface CreateReviewResponse {
  review: ReviewRecord
  run: ReviewRunRecord
}

export interface RemoteAgentSettings {
  configured: boolean
  preferredTool: RemoteAgentPreferredTool
  host?: string
  user?: string
  port?: number
  shellPrelude?: string
  reviewFollowUp?: RemoteAgentReviewFollowUpSettings
}

export interface RemoteAgentReviewFollowUpSettings {
  enabled: boolean
  mainUser?: string
  defaultReviewPrompt?: string
}

export interface RemoteCleanupSummary {
  closedTasksCleaned: number
  missingTasksCleaned: number
  localDispatchHistoriesRemoved: number
  remoteWorktreesRemoved: number
  remoteRunDirectoriesRemoved: number
}

export interface RemoteCleanupResponse {
  summary: RemoteCleanupSummary
}

export interface RemoteResetSummary {
  workspaceEntriesRemoved: number
  registryRemoved: boolean
}

export interface RemoteResetResponse {
  summary: RemoteResetSummary
}

export interface TaskChangeVersionResponse {
  version: number
}

export interface ProjectsResponse {
  projects: ProjectInfo[]
}

export interface TasksResponse {
  tasks: Task[]
}

export type Priority = 'high' | 'medium' | 'low'
export type Status = 'open' | 'closed'
export type DispatchStatus = 'preparing' | 'running' | 'succeeded' | 'canceled' | 'failed' | 'blocked'

export const REMOTE_AGENT_TOOLS = {
  CODEX: 'codex',
  CLAUDE: 'claude',
} as const

export type RemoteAgentPreferredTool = 'codex' | 'claude'

export function isRemoteAgentPreferredTool(value: unknown): value is RemoteAgentPreferredTool {
  return value === REMOTE_AGENT_TOOLS.CODEX || value === REMOTE_AGENT_TOOLS.CLAUDE
}

export interface ProjectInfo {
  canonicalName: string
  aliases: string[]
  metadata: ProjectMetadata
}

export interface ProjectMetadata {
  repoUrl: string
  gitUrl: string
  baseBranch: string
  description?: string
}

export interface Task {
  id: string
  project: string
  priority: Priority
  status: Status
  description: string
  createdAt: string
  updatedAt: string
  source?: 'cli' | 'web'
}

export interface TaskCreateInput {
  project: string
  priority: Priority
  description: string
}

export interface TaskFollowUpInput {
  request: string
}

export interface TaskDispatchInput {
  preferredTool?: RemoteAgentPreferredTool
}

export interface ReviewFollowUpInput {
  request: string
}

export interface TaskDispatch {
  dispatchId: string
  taskId: string
  preferredTool: RemoteAgentPreferredTool
  project: string
  status: DispatchStatus
  createdAt: string
  updatedAt: string
  finishedAt?: string
  remoteHost: string
  branchName?: string
  worktreePath?: string
  pullRequestUrl?: string
  followUpRequest?: string
  summary?: string
  notes?: string
  errorMessage?: string
}

export interface ReviewRecord {
  id: string
  pullRequestUrl: string
  pullRequestNumber: number
  pullRequestTitle: string
  repositoryFullName: string
  repoUrl: string
  gitUrl: string
  baseBranch: string
  workspaceKey: string
  preferredTool: RemoteAgentPreferredTool
  project?: string
  mainUser: string
  defaultReviewPrompt?: string
  extraInstructions?: string
  createdAt: string
  updatedAt: string
}

export interface ReviewRunRecord {
  dispatchId: string
  reviewId: string
  pullRequestUrl: string
  repositoryFullName: string
  workspaceKey: string
  preferredTool: RemoteAgentPreferredTool
  status: DispatchStatus
  createdAt: string
  updatedAt: string
  finishedAt?: string
  remoteHost: string
  branchName?: string
  worktreePath?: string
  followUpRequest?: string
  targetHeadOid?: string
  summary?: string
  reviewSubmitted: boolean
  githubReviewId?: string
  githubReviewUrl?: string
  notes?: string
  errorMessage?: string
}

export interface ReviewSummary {
  review: ReviewRecord
  latestRun?: ReviewRunRecord
}

export interface RunRecord {
  task: Task
  dispatch: TaskDispatch
}

export interface CreateReviewInput {
  pullRequestUrl: string
  preferredTool?: RemoteAgentPreferredTool
  extraInstructions?: string
}

export interface TaskUpdateInput {
  description?: string
  priority?: Priority
  status?: Status
}

export interface ProjectMetadataUpdateInput {
  repoUrl: string
  gitUrl: string
  baseBranch: string
  description?: string
}

export interface RemoteAgentSettingsUpdateInput {
  preferredTool: RemoteAgentPreferredTool
  shellPrelude: string
  reviewFollowUp?: RemoteAgentReviewFollowUpSettings
}
