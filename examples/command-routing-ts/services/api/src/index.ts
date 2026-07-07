// Worker "api" — the PUSH side of command delivery.
//
// A Worker registers commands with the SDK `command()` registrar. The platform
// pushes each invocation to the Worker as an HTTP request. This Worker also
// serves a normal HTTP app (the default export).
//
// It registers `status` and `search` — the SAME names the indexer daemon
// registers. A caller tells them apart with `.target("api")`.

import { command, kv } from "@alienplatform/sdk"
import { Hono } from "hono"
import { countDocs, searchIndex } from "../../../shared/scan-all"

const RESOURCE = "api"

// Overlapping command #1: `status`. The Worker answers from the request that
// was pushed to it, so `role` is "worker".
command("status", async () => ({
  resource: RESOURCE,
  role: "worker",
  model: "push",
  documents: await countDocs(kv("index")),
  at: new Date().toISOString(),
}))

// Overlapping command #2: `search`. Reads the shared index the daemon builds.
command("search", async ({ term }: { term: string }) => ({
  resource: RESOURCE,
  term,
  hits: await searchIndex(kv("index"), term),
}))

const app = new Hono()
app.get("/health", c => c.json({ status: "ok", resource: RESOURCE }))
app.get("/status", async c =>
  c.json({ resource: RESOURCE, documents: await countDocs(kv("index")) }),
)

export default app
