import { access, mkdir, readdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises'
import { constants as fsConstants } from 'node:fs'
import { dirname, join } from 'node:path'

import matter from 'gray-matter'

import {
  TASK_FILE_EXTENSION,
  taskCreateInputSchema,
  taskSchema,
  taskUpdateInputSchema,
  type Status,
  type Task,
  type TaskCreateInput,
  type TaskUpdateInput,
} from '@track/shared'

import { TrackError } from '../errors'
import { getDataDir } from '../utils/path-utils'
import { buildUniqueTaskId } from '../utils/task-id'

// =============================================================================
// Filesystem Repository
// =============================================================================
//
// The repository treats Markdown files as the source of truth on disk. That is
// why the code favors explicit file reads and rewrites over clever incremental
// updates: a human should be able to open a task file, understand it, edit it,
// and trust that the app will honor those edits.
//
interface TaskFileRecord {
  filePath: string
  task: Task
}

async function pathExists(pathValue: string): Promise<boolean> {
  try {
    await access(pathValue, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

function normalizeFrontmatterValue(value: unknown): string | undefined {
  if (value instanceof Date) {
    return value.toISOString()
  }

  if (typeof value === 'string') {
    return value
  }

  return undefined
}

function extractTaskDescription(parsedFile: matter.GrayMatterFile<string>): string | undefined {
  // The body is the most natural place for a human to edit the task text, so we
  // intentionally prefer it over the mirrored frontmatter copy when reading.
  const bodyDescription = parsedFile.content.trim()
  if (bodyDescription.length > 0) {
    return bodyDescription
  }

  return normalizeFrontmatterValue(parsedFile.data.description)
}

export class FileTaskRepository {
  private readonly dataDir: string

  constructor(options?: { dataDir?: string }) {
    this.dataDir = getDataDir(options?.dataDir)
  }

  getResolvedDataDir(): string {
    return this.dataDir
  }

  async listTasks(filters?: { includeClosed?: boolean; project?: string }): Promise<Task[]> {
    if (!(await pathExists(this.dataDir))) {
      return []
    }

    // Listing is deliberately forgiving because manual editing is part of the
    // product story. One broken file should not hide every healthy task.
    const projects = filters?.project ? [filters.project] : await this.listProjectDirectories()
    const statuses: Status[] = filters?.includeClosed ? ['open', 'closed'] : ['open']
    const tasks: Task[] = []

    for (const project of projects) {
      for (const status of statuses) {
        const directoryPath = this.getStatusDirectory(project, status)
        if (!(await pathExists(directoryPath))) {
          continue
        }

        const files = await readdir(directoryPath)
        for (const fileName of files) {
          if (!fileName.endsWith(TASK_FILE_EXTENSION)) {
            continue
          }

          const filePath = join(directoryPath, fileName)

          try {
            const record = await this.readTaskFile(filePath)
            tasks.push(record.task)
          } catch (error) {
            console.warn(
              `Skipping malformed task file at ${filePath}: ${error instanceof Error ? error.message : 'Unknown error'}`,
            )
          }
        }
      }
    }

    return tasks
  }

  async createTask(input: TaskCreateInput): Promise<{ filePath: string; task: Task }> {
    const parsedInput = taskCreateInputSchema.parse(input)
    const now = new Date()

    // We create the project folders on demand so a new task can be the first
    // artifact under a discovered project without any separate setup step.
    await this.ensureProjectDirectories(parsedInput.project)

    const destinationDirectory = this.getStatusDirectory(parsedInput.project, 'open')
    const id = await buildUniqueTaskId({
      date: now,
      description: parsedInput.description,
      exists: async (candidateId) => pathExists(join(destinationDirectory, `${candidateId}${TASK_FILE_EXTENSION}`)),
    })

    const task: Task = {
      id,
      project: parsedInput.project,
      priority: parsedInput.priority,
      status: 'open',
      description: parsedInput.description,
      createdAt: now.toISOString(),
      updatedAt: now.toISOString(),
      source: parsedInput.source,
    }

    const filePath = this.getTaskFilePath(parsedInput.project, 'open', task.id)
    await this.writeTaskFile(filePath, task)

    return { task, filePath }
  }

  async updateTask(id: string, input: TaskUpdateInput): Promise<Task> {
    const parsedInput = taskUpdateInputSchema.parse(input)
    const existingRecord = await this.findTaskById(id)

    // Status changes are expressed as regular updates so the caller does not
    // need separate "close" and "reopen" repository concepts.
    const nextStatus = parsedInput.status ?? existingRecord.task.status
    const updatedTask: Task = {
      ...existingRecord.task,
      description: parsedInput.description ?? existingRecord.task.description,
      priority: parsedInput.priority ?? existingRecord.task.priority,
      status: nextStatus,
      updatedAt: new Date().toISOString(),
    }

    const destinationFilePath = this.getTaskFilePath(updatedTask.project, nextStatus, updatedTask.id)
    await this.ensureProjectDirectories(updatedTask.project)

    // We rewrite the full Markdown file even for small edits so the on-disk
    // representation always remains a complete, hand-editable source of truth.
    await this.writeTaskFile(destinationFilePath, updatedTask)

    if (existingRecord.filePath !== destinationFilePath) {
      await rm(existingRecord.filePath, { force: true })
    }

    return updatedTask
  }

  async deleteTask(id: string): Promise<void> {
    const existingRecord = await this.findTaskById(id)
    await rm(existingRecord.filePath)
  }

  private async findTaskById(id: string): Promise<TaskFileRecord> {
    if (!(await pathExists(this.dataDir))) {
      throw new TrackError('TASK_NOT_FOUND', `Task ${id} was not found.`, { status: 404 })
    }

    // IDs are filename stems, so we can recover a task from the filesystem
    // without forcing the frontend to remember project and status as extra
    // mutation parameters.
    const projectDirectories = await this.listProjectDirectories()
    for (const project of projectDirectories) {
      for (const status of ['open', 'closed'] as const) {
        const filePath = this.getTaskFilePath(project, status, id)
        if (!(await pathExists(filePath))) {
          continue
        }

        return this.readTaskFile(filePath)
      }
    }

    throw new TrackError('TASK_NOT_FOUND', `Task ${id} was not found.`, { status: 404 })
  }

  private async listProjectDirectories(): Promise<string[]> {
    const entries = await readdir(this.dataDir, { withFileTypes: true }).catch(() => [])
    return entries.filter((entry) => entry.isDirectory()).map((entry) => entry.name)
  }

  private async ensureProjectDirectories(project: string): Promise<void> {
    await mkdir(this.getStatusDirectory(project, 'open'), { recursive: true })
    await mkdir(this.getStatusDirectory(project, 'closed'), { recursive: true })
  }

  private getStatusDirectory(project: string, status: Status): string {
    return join(this.dataDir, project, status)
  }

  private getTaskFilePath(project: string, status: Status, id: string): string {
    return join(this.getStatusDirectory(project, status), `${id}${TASK_FILE_EXTENSION}`)
  }

  private async readTaskFile(filePath: string): Promise<TaskFileRecord> {
    const rawFile = await readFile(filePath, 'utf8')
    const parsedFile = matter(rawFile)

    // Reading normalizes the editable Markdown shape back into the stricter task
    // schema used by the app and API responses.
    const parsedTask = taskSchema.parse({
      id: normalizeFrontmatterValue(parsedFile.data.id),
      project: normalizeFrontmatterValue(parsedFile.data.project),
      priority: normalizeFrontmatterValue(parsedFile.data.priority),
      status: normalizeFrontmatterValue(parsedFile.data.status),
      description: extractTaskDescription(parsedFile),
      createdAt: normalizeFrontmatterValue(parsedFile.data.createdAt),
      updatedAt: normalizeFrontmatterValue(parsedFile.data.updatedAt),
      source: normalizeFrontmatterValue(parsedFile.data.source),
    })

    return {
      filePath,
      task: parsedTask,
    }
  }

  private async writeTaskFile(filePath: string, task: Task): Promise<void> {
    const frontmatter = {
      id: task.id,
      project: task.project,
      priority: task.priority,
      status: task.status,
      description: task.description.trim(),
      createdAt: task.createdAt,
      updatedAt: task.updatedAt,
      ...(task.source ? { source: task.source } : {}),
    }

    // We intentionally mirror the description into both frontmatter and body:
    // frontmatter keeps metadata scanners simple, while the body keeps the file
    // pleasant to read and edit in a text editor.
    const serializedTask = matter.stringify(task.description.trim(), frontmatter).trimEnd() + '\n'

    // The temp-file rename keeps edits best-effort atomic without introducing
    // heavier coordination that this single-user tool does not need yet.
    const tempFilePath = `${filePath}.tmp`
    await mkdir(dirname(filePath), { recursive: true })
    await writeFile(tempFilePath, serializedTask, 'utf8')
    await rename(tempFilePath, filePath)

    const fileStats = await stat(filePath)
    if (!fileStats.isFile()) {
      throw new TrackError('TASK_WRITE_FAILED', `Could not write task file at ${filePath}.`)
    }
  }
}
