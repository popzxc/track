import type { Hono } from 'hono'

export function registerHealthRoutes(app: Hono) {
  app.get('/health', (context) => context.json({ ok: true }))
}
