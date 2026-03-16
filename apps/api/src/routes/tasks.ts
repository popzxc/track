import type { Hono } from 'hono'

import { TrackError } from '@track/core'
import { taskUpdateInputSchema } from '@track/shared'

import type { AppDependencies } from '../app'

export function registerTaskRoutes(app: Hono, dependencies: AppDependencies) {
  app.get('/api/tasks', async (context) => {
    const includeClosed = context.req.query('includeClosed') === 'true'
    const project = context.req.query('project') || undefined
    const tasks = await dependencies.taskService.listTasks({
      includeClosed,
      project,
    })

    return context.json({ tasks })
  })

  app.patch('/api/tasks/:id', async (context) => {
    const id = context.req.param('id')
    const body = await context.req.json().catch(() => {
      throw new TrackError('INVALID_JSON', 'Request body is not valid JSON.', { status: 400 })
    })
    const parsedInput = taskUpdateInputSchema.safeParse(body)
    if (!parsedInput.success) {
      throw new TrackError(
        'INVALID_TASK_UPDATE',
        parsedInput.error.issues[0]?.message ?? 'Invalid task update payload.',
        { status: 400 },
      )
    }

    const updateInput = parsedInput.data
    const updatedTask = await dependencies.taskService.updateTask(id, updateInput)

    return context.json(updatedTask)
  })

  app.delete('/api/tasks/:id', async (context) => {
    const id = context.req.param('id')
    await dependencies.taskService.deleteTask(id)

    return context.json({ ok: true })
  })
}
