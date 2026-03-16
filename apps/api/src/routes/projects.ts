import type { Hono } from 'hono'

import type { AppDependencies } from '../app'

export function registerProjectRoutes(app: Hono, dependencies: AppDependencies) {
  app.get('/api/projects', async (context) => {
    const config = await dependencies.configService.loadConfig()
    const projects = await dependencies.projectService.discoverProjects(config)

    return context.json({ projects })
  })
}
