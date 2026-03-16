import type { TaskUpdateInput } from '@track/shared'

import { FileTaskRepository } from '../storage/file-task-repository'
import { sortTasks } from '../utils/task-sort'

export class TaskService {
  constructor(private readonly taskRepository: FileTaskRepository) {}

  async listTasks(filters?: { includeClosed?: boolean; project?: string }) {
    const tasks = await this.taskRepository.listTasks(filters)
    return sortTasks(tasks)
  }

  async updateTask(id: string, input: TaskUpdateInput) {
    return this.taskRepository.updateTask(id, input)
  }

  async deleteTask(id: string) {
    await this.taskRepository.deleteTask(id)
  }
}
