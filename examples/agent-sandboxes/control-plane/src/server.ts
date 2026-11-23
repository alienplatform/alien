// Control plane: the vendor's web app. Runs anywhere; it reaches the deployment only through
// the deployment's command channel.
//
//   ALIEN_MANAGER_URL=... ALIEN_DEPLOYMENT_ID=... ALIEN_TOKEN=... bun src/server.ts

import { readFileSync } from "node:fs"
import { Hono } from "hono"
import { streamSSE } from "hono/streaming"
import { commandsDataPlane } from "./data-plane"
import { ThreadRouter } from "./router"
import { ThreadStore } from "./store"

function required(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`Missing required environment variable ${name}`)
  return value
}

const dp = commandsDataPlane({
  managerUrl: required("ALIEN_MANAGER_URL"),
  deploymentId: required("ALIEN_DEPLOYMENT_ID"),
  token: required("ALIEN_TOKEN"),
})
const store = new ThreadStore(process.env.THREADS_FILE ?? "threads.json")
const router = new ThreadRouter(dp, store, {
  idlePauseMs: Number(process.env.IDLE_PAUSE_SECONDS ?? 120) * 1000,
})

const page = (name: string) => readFileSync(new URL(`../public/${name}`, import.meta.url), "utf8")
const app = new Hono()

app.get("/", c => c.html(page("index.html")))
app.get("/app.js", c => c.body(page("app.js"), 200, { "content-type": "text/javascript" }))

app.get("/api/threads", c => c.json({ threads: store.list() }))

app.post("/api/threads", async c => {
  const body = await c.req.json<{ title?: string; user?: string }>()
  return c.json(router.create(body.title || "New thread", body.user || "alice"))
})

app.post("/api/threads/:id/messages", async c => {
  const body = await c.req.json<{ user: string; text: string }>()
  try {
    await router.send(c.req.param("id"), body.user, body.text)
    return c.json({ started: true })
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : String(error) }, 409)
  }
})

app.post("/api/threads/:id/exec", async c => {
  const body = await c.req.json<{ command: string }>()
  try {
    await router.exec(c.req.param("id"), body.command)
    return c.json({ ok: true })
  } catch (error) {
    return c.json({ error: error instanceof Error ? error.message : String(error) }, 409)
  }
})

app.post("/api/threads/:id/approvals/:approval", async c => {
  const body = await c.req.json<{ approve: boolean; by: string }>()
  return c.json(await router.decide(c.req.param("id"), c.req.param("approval"), body.approve, body.by))
})

app.get("/api/threads/:id/events", c => {
  const id = c.req.param("id")
  return streamSSE(c, async stream => {
    for (const event of router.history(id)) await stream.writeSSE({ data: JSON.stringify(event) })
    const queue: string[] = []
    let wake: (() => void) | undefined
    const unsubscribe = router.subscribe(id, event => {
      queue.push(JSON.stringify(event))
      wake?.()
    })
    stream.onAbort(unsubscribe)
    while (!stream.aborted) {
      if (queue.length === 0) {
        await new Promise<void>(resolve => {
          wake = resolve
          setTimeout(resolve, 15_000)
        })
        wake = undefined
        if (queue.length === 0) await stream.writeSSE({ event: "ping", data: "" })
      }
      while (queue.length > 0) await stream.writeSSE({ data: queue.shift() as string })
    }
  })
})

const port = Number(process.env.PORT ?? 3000)
console.log(`control plane on http://localhost:${port}`)

export default { port, fetch: app.fetch, idleTimeout: 0 }
