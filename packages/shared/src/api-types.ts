import type { ProjectInfo } from './project-types'
import type { Task } from './task-types'

export interface HealthResponse {
  ok: true
}

export interface ProjectsResponse {
  projects: ProjectInfo[]
}

export interface TasksResponse {
  tasks: Task[]
}

export interface DeleteTaskResponse {
  ok: true
}

export interface ApiErrorResponse {
  error: {
    code: string
    message: string
  }
}
