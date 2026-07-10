// Daemon "indexer-daemon" — the PULL side of command delivery.
//
// A Daemon is a resident process. It cannot receive pushed HTTP invocations, so
// it LEASES commands from the command server over outbound HTTPS using the
// `@alienplatform/commands` receiver. It also uses a `kv` binding directly (the
// same in-process bindings a Container/Daemon gets — no gRPC, no runtime).
//
// It registers `status` and `search` — the SAME names the api Worker registers.
// A caller reaches this one with `.target("indexer-daemon")`.

import { kv } from "@alienplatform/bindings"
import { createCommandReceiver } from "@alienplatform/commands"
import { countDocs, searchIndex } from "../../../shared/scan-all"

const RESOURCE = "indexer-daemon"
const index = kv("index")

const SEED_DOCS: Record<string, string> = {
  "getting-started": "How to deploy your first Alien stack",
  commands: "Invoke commands on a deployment by target resource id",
  bindings: "Storage, kv, queue, and vault bindings run in-process",
  daemons: "A daemon is a resident process that leases commands",
}

// Background work that justifies this being a daemon: keep the shared index
// populated. In a real agent this would crawl a source; here it seeds a handful
// of documents the `search` command (on either resource) can read.
async function buildIndex(signal: AbortSignal): Promise<void> {
  while (!signal.aborted) {
    for (const [id, text] of Object.entries(SEED_DOCS)) {
      await index.set(`doc:${id}`, text)
    }
    await new Promise(resolve => setTimeout(resolve, 30_000))
  }
}

const controller = new AbortController()
void buildIndex(controller.signal).catch(error => {
  console.error("indexer loop failed", error)
})

const receiver = createCommandReceiver()

// Overlapping command #1: `status`. Answered by the daemon, so `role` is
// "daemon" and `model` is "pull".
receiver.handle("status", async () => ({
  resource: RESOURCE,
  role: "daemon",
  model: "pull",
  documents: await countDocs(index),
  at: new Date().toISOString(),
}))

// Overlapping command #2: `search`, reading the index this daemon maintains.
receiver.handle("search", async ctx => {
  const { term } = JSON.parse(new TextDecoder().decode(ctx.input)) as { term: string }
  return { resource: RESOURCE, term, hits: await searchIndex(index, term) }
})

console.log(`${RESOURCE} leasing commands`)

// A source-built daemon compiles to a single binary that embeds the bindings
// native addon, which requires a CommonJS bundle — and CommonJS forbids
// top-level await. Drive the receiver from an async entry point instead.
async function main(): Promise<void> {
  await receiver.run()
  controller.abort()
}

void main()
