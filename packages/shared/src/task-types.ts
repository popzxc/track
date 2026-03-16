export type Priority = 'high' | 'medium' | 'low'
export type Status = 'open' | 'closed'
export type TaskSource = 'cli' | 'web'

export interface Task {
  id: string
  project: string
  priority: Priority
  status: Status
  description: string
  createdAt: string
  updatedAt: string
  source?: TaskSource
}

export interface TaskUpdateInput {
  description?: string
  priority?: Priority
  status?: Status
}

export interface TaskCreateInput {
  project: string
  priority: Priority
  description: string
  source?: TaskSource
}

export interface ParsedTaskCandidate {
  project: string | null
  priority: Priority
  description: string
  confidence: 'high' | 'low'
  reason?: string
}
