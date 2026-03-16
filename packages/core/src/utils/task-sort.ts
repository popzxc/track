import type { Task } from '@track/shared'
import { PRIORITY_RANK, STATUS_RANK } from '@track/shared'

export function sortTasks(tasks: Task[]): Task[] {
  return [...tasks].sort((left, right) => {
    const statusDifference = STATUS_RANK[left.status] - STATUS_RANK[right.status]
    if (statusDifference !== 0) {
      return statusDifference
    }

    const priorityDifference = PRIORITY_RANK[left.priority] - PRIORITY_RANK[right.priority]
    if (priorityDifference !== 0) {
      return priorityDifference
    }

    return right.createdAt.localeCompare(left.createdAt)
  })
}
