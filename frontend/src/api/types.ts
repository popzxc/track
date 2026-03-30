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

export type MigrationState = 'ready' | 'import_required' | 'imported'

export interface LegacyScanSummary {
  projectsFound: number
  aliasesFound: number
  tasksFound: number
  taskDispatchesFound: number
  reviewsFound: number
  reviewRunsFound: number
  remoteAgentConfigured: boolean
}

export interface SkippedLegacyRecord {
  kind: string
  path: string
  error: string
}

export interface CleanupCandidate {
  path: string
  reason: string
}

export interface MigrationStatus {
  state: MigrationState
  requiresMigration: boolean
  canImport: boolean
  legacyDetected: boolean
  summary: LegacyScanSummary
  skippedRecords: SkippedLegacyRecord[]
  cleanupCandidates: CleanupCandidate[]
}

export interface MigrationImportSummary {
  importedProjects: number
  importedAliases: number
  importedTasks: number
  importedTaskDispatches: number
  importedReviews: number
  importedReviewRuns: number
  remoteAgentConfigImported: boolean
  copiedSecretFiles: string[]
  skippedRecords: SkippedLegacyRecord[]
  cleanupCandidates: CleanupCandidate[]
}

export interface MigrationStatusResponse {
  migration: MigrationStatus
}

export interface MigrationImportResponse {
  summary: MigrationImportSummary
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

export interface ReviewFollowUpInput {
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
  shellPrelude: string
  reviewFollowUp?: RemoteAgentReviewFollowUpSettings
}
