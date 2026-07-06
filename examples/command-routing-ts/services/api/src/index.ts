// Worker "api" — the PUSH side of command delivery.
//
// A Worker registers commands with the SDK `command()` registrar. The platform
// pushes each invocation to the Worker as an HTTP request. This Worker also
// serves a normal HTTP app (the default export).
//
// It registers `status` and `search` — the SAME names the indexer daemon
// registers. A caller tells them apart with `.target("api")`.

import { command, kv } from "@alienplatform/sdk"
import type { Kv } from "@alienplatform/sdk"
import { Hono } from "hono"

const RESOURCE = "api"

/** Iterate every key under a prefix, following the scan cursor across pages. */
async function* scanAll(store: Kv, prefix: string) {
  let cursor: string | undefined
  do {
    const page = await store.scan(prefix, undefined, cursor)
    for (const item of page.items) yield item
    cursor = page.nextCursor
  } while (cursor)
}

async function indexSize(): Promise<number> {
  const store = kv("index")
  let count = 0
  for await (const _ of scanAll(store, "doc:")) count++
  return count
}

// Overlapping command #1: `status`. The Worker answers from the request that
// was pushed to it, so `role` is "worker".
command("status", async () => ({
  resource: RESOURCE,
  role: "worker",
  model: "push",
  documents: await indexSize(),
  at: new Date().toISOString(),
}))

// Overlapping command #2: `search`. Reads the shared index the daemon builds.
command("search", async ({ term }: { term: string }) => {
  const store = kv("index")
  const hits: string[] = []
  for await (const entry of scanAll(store, "doc:")) {
    const text = new TextDecoder().decode(entry.value)
    if (text.toLowerCase().includes(term.toLowerCase())) {
      hits.push(entry.key.slice("doc:".length))
    }
  }
  return { resource: RESOURCE, term, hits }
})

const app = new Hono()
app.get("/health", c => c.json({ status: "ok", resource: RESOURCE }))
app.get("/status", async c => c.json({ resource: RESOURCE, documents: await indexSize() }))

export default app
