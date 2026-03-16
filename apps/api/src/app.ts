import { access } from 'node:fs/promises'
import { constants as fsConstants } from 'node:fs'
import { join, normalize } from 'node:path'

import { Hono } from 'hono'

import { ConfigService, FileTaskRepository, ProjectService, TaskService, TrackError, isTrackError } from '@track/core'

import { registerHealthRoutes } from './routes/health'
import { registerProjectRoutes } from './routes/projects'
import { registerTaskRoutes } from './routes/tasks'

// =============================================================================
// Unified Server Surface
// =============================================================================
//
// Even though the backend is primarily a JSON API, the deployed app uses this
// same process to serve the built frontend so Docker can expose a single port.
// That keeps local hosting and container hosting aligned.
//
export interface AppDependencies {
  configService: ConfigService
  projectService: ProjectService
  taskService: TaskService
  staticRoot: string
}

async function fileExists(pathValue: string): Promise<boolean> {
  try {
    await access(pathValue, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}

function jsonErrorResponse(error: unknown) {
  if (isTrackError(error)) {
    return {
      status: error.status,
      body: {
        error: {
          code: error.code,
          message: error.message,
        },
      },
    }
  }

  return {
    status: 500,
    body: {
      error: {
        code: 'INTERNAL_ERROR',
        message: error instanceof Error ? error.message : 'An unexpected error occurred.',
      },
    },
  }
}

async function serveStaticAsset(requestPath: string, staticRoot: string) {
  // Single-page-app routing means unknown frontend paths should still fall back
  // to `index.html` as long as the built asset directory exists.
  const normalizedPath = normalize(requestPath).replace(/^(\.\.(\/|\\|$))+/, '')
  const relativePath = normalizedPath === '/' ? 'index.html' : normalizedPath.replace(/^\/+/, '')
  const assetPath = join(staticRoot, relativePath)

  if (await fileExists(assetPath)) {
    return new Response(Bun.file(assetPath))
  }

  const indexPath = join(staticRoot, 'index.html')
  if (await fileExists(indexPath)) {
    return new Response(Bun.file(indexPath))
  }

  return new Response('Static assets are not available yet.', { status: 404 })
}

export function createDefaultDependencies(): AppDependencies {
  // The default dependency graph stays tiny on purpose so tests can swap in
  // temp-directory repositories without booting the entire application stack.
  const configService = new ConfigService()
  const projectService = new ProjectService()
  const taskRepository = new FileTaskRepository()
  const taskService = new TaskService(taskRepository)

  return {
    configService,
    projectService,
    taskService,
    staticRoot: join(import.meta.dir, '../public'),
  }
}

export function createApp(dependencies: AppDependencies) {
  const app = new Hono()

  app.onError((error, context) => {
    const response = jsonErrorResponse(error)
    return context.json(response.body, response.status as 400 | 404 | 500)
  })

  registerHealthRoutes(app)
  registerProjectRoutes(app, dependencies)
  registerTaskRoutes(app, dependencies)

  // We serve the frontend from this same server so the deployed Docker image
  // can expose one port for both the REST API and the local task UI. The
  // Dockerfile copies the built web assets into `apps/api/public` at build time.
  app.get('*', async (context) => {
    if (context.req.path.startsWith('/api/') || context.req.path === '/health') {
      throw new TrackError('ROUTE_NOT_FOUND', `Route ${context.req.path} was not found.`, { status: 404 })
    }

    return serveStaticAsset(context.req.path, dependencies.staticRoot)
  })

  return app
}
