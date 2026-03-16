import { createApp, createDefaultDependencies } from './app'

const port = Number(process.env.PORT ?? '3210')
const app = createApp(createDefaultDependencies())

if (import.meta.main) {
  Bun.serve({
    port,
    fetch: app.fetch,
  })

  console.log(`track API listening on http://localhost:${port}`)
}

export default app
