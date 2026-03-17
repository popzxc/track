export interface ApiErrorResponse {
  error: {
    code: string
    message: string
  }
}

export interface DeleteTaskResponse {
  ok: true
}

export interface ProjectsResponse {
  projects: ProjectInfo[]
}

export interface TasksResponse {
  tasks: Task[]
}

export type Priority = 'high' | 'medium' | 'low'
export type Status = 'open' | 'closed'

export interface ProjectInfo {
  canonicalName: string
  path: string
  aliases: string[]
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

export interface TaskUpdateInput {
  description?: string
  priority?: Priority
  status?: Status
}
