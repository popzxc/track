import type {
  DeleteTaskResponse,
  DispatchesResponse,
  ProjectInfo,
  ProjectMetadataUpdateInput,
  ProjectsResponse,
  RemoteCleanupResponse,
  RemoteCleanupSummary,
  RemoteResetResponse,
  RemoteResetSummary,
  RemoteAgentSettings,
  RemoteAgentSettingsUpdateInput,
  RunRecord,
  RunsResponse,
  Task,
  TaskCreateInput,
  TaskDispatch,
  TaskFollowUpInput,
  TaskChangeVersionResponse,
  TaskUpdateInput,
  TasksResponse,
} from './types'

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

export async function updateProject(
  canonicalName: string,
  input: ProjectMetadataUpdateInput,
): Promise<ProjectInfo> {
  return readJson<ProjectInfo>(`/api/projects/${encodeURIComponent(canonicalName)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
  })
}

export async function fetchRemoteAgentSettings(): Promise<RemoteAgentSettings> {
  return readJson<RemoteAgentSettings>('/api/remote-agent')
}

export async function updateRemoteAgentSettings(
  input: RemoteAgentSettingsUpdateInput,
): Promise<RemoteAgentSettings> {
  return readJson<RemoteAgentSettings>('/api/remote-agent', {
    method: 'PATCH',
    body: JSON.stringify(input),
  })
}

export async function cleanupRemoteAgentArtifacts(): Promise<RemoteCleanupSummary> {
  const response = await readJson<RemoteCleanupResponse>('/api/remote-agent/cleanup', {
    method: 'POST',
  })

  return response.summary
}

export async function resetRemoteAgentWorkspace(): Promise<RemoteResetSummary> {
  const response = await readJson<RemoteResetResponse>('/api/remote-agent/reset', {
    method: 'POST',
  })

  return response.summary
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

export async function createTask(input: TaskCreateInput): Promise<Task> {
  return readJson<Task>('/api/tasks', {
    method: 'POST',
    body: JSON.stringify(input),
  })
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

export async function dispatchTask(id: string): Promise<TaskDispatch> {
  return readJson<TaskDispatch>(`/api/tasks/${id}/dispatch`, {
    method: 'POST',
  })
}

export async function followUpTask(id: string, input: TaskFollowUpInput): Promise<TaskDispatch> {
  return readJson<TaskDispatch>(`/api/tasks/${id}/follow-up`, {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export async function cancelDispatch(id: string): Promise<TaskDispatch> {
  return readJson<TaskDispatch>(`/api/tasks/${id}/dispatch/cancel`, {
    method: 'POST',
  })
}

export async function discardDispatch(id: string): Promise<DeleteTaskResponse> {
  return readJson<DeleteTaskResponse>(`/api/tasks/${id}/dispatch`, {
    method: 'DELETE',
  })
}

export async function fetchDispatches(taskIds: string[]): Promise<TaskDispatch[]> {
  if (taskIds.length === 0) {
    return []
  }

  const query = new URLSearchParams()
  for (const taskId of taskIds) {
    query.append('taskId', taskId)
  }

  const response = await readJson<DispatchesResponse>(`/api/dispatches?${query.toString()}`)
  return response.dispatches
}

export async function fetchRuns(limit = 200): Promise<RunRecord[]> {
  const response = await readJson<RunsResponse>(`/api/runs?limit=${limit}`)
  return response.runs
}

export async function fetchTaskChangeVersion(): Promise<number> {
  const response = await readJson<TaskChangeVersionResponse>('/api/events/version')
  return response.version
}

export { ApiClientError }
