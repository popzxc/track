import {
  ConfigService,
  createTaskParser,
  FileTaskRepository,
  ProjectService,
  TaskCaptureService,
  TrackError,
} from '@track/core'

import { formatCreatedTaskOutput } from '../output/format'

export async function runCreateCommand(argv: string[]) {
  const rawText = argv.join(' ').trim()
  if (rawText.length === 0) {
    throw new TrackError('EMPTY_INPUT', 'Please provide a task description.')
  }

  const configService = new ConfigService()
  const config = await configService.loadConfig()

  const taskCaptureService = new TaskCaptureService({
    aiTaskParser: createTaskParser(config),
    configService,
    projectService: new ProjectService(),
    taskRepository: new FileTaskRepository(),
  })

  const createdTask = await taskCaptureService.createTaskFromText({
    rawText,
    source: 'cli',
  })

  return formatCreatedTaskOutput(createdTask)
}
