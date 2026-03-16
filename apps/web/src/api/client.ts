import type { DeleteTaskResponse, ProjectsResponse, TasksResponse } from '@track/shared'
import type { Task, TaskUpdateInput, ProjectInfo } from '@track/shared'

class ApiClientError extends Error {
  readonly code: string

  constructor(code: string, message: string) {
    super(message)
    this.name = 'ApiClientError'
    this.code = code
  }
}

async function readJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    headers: {
      'content-type': 'application/json',
      ...(init?.headers ?? {}),
    },
    ...init,
  })

  const json = await response.json().catch(() => ({}))
  if (!response.ok) {
    throw new ApiClientError(json?.error?.code ?? 'API_ERROR', json?.error?.message ?? 'Request failed.')
  }

  return json as T
}

export async function fetchProjects(): Promise<ProjectInfo[]> {
  const response = await readJson<ProjectsResponse>('/api/projects')
  return response.projects
}

export async function fetchTasks(options: { includeClosed: boolean; project?: string }): Promise<Task[]> {
  const query = new URLSearchParams()
  if (options.includeClosed) {
    query.set('includeClosed', 'true')
  }

  if (options.project) {
    query.set('project', options.project)
  }

  const queryString = query.toString()
  const response = await readJson<TasksResponse>(queryString.length > 0 ? `/api/tasks?${queryString}` : '/api/tasks')
  return response.tasks
}

export async function updateTask(id: string, input: TaskUpdateInput): Promise<Task> {
  return readJson<Task>(`/api/tasks/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
  })
}

export async function deleteTask(id: string): Promise<DeleteTaskResponse> {
  return readJson<DeleteTaskResponse>(`/api/tasks/${id}`, {
    method: 'DELETE',
  })
}

export { ApiClientError }
