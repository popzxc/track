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

export interface RemoteAgentSettings {
  configured: boolean
  host?: string
  user?: string
  port?: number
  shellPrelude?: string
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

export interface ProjectInfo {
  canonicalName: string
  path: string
  aliases: string[]
  metadata?: ProjectMetadata
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

export interface TaskDispatch {
  dispatchId: string
  taskId: string
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

export interface RunRecord {
  task: Task
  dispatch: TaskDispatch
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
  shellPrelude: string
}
