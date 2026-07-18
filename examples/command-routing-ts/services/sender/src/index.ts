// Sender — invokes the SAME command name on two different resources by target.
//
// Run this against a deployment of the command-routing stack. It reads the
// deployment's command endpoint, id, and token from the environment, then calls
// `status` twice: once targeting the Worker, once targeting the Daemon. The two
// responses differ (worker/push vs daemon/pull), proving the command server
// routes by target resource id rather than by command name.
//
//   ALIEN_MANAGER_URL=... ALIEN_DEPLOYMENT_ID=... ALIEN_TOKEN=... \
//     bun src/index.ts

import { CommandsClient } from "@alienplatform/commands"

function required(name: string): string {
  const value = process.env[name]
  if (!value) {
    throw new Error(`Missing required environment variable ${name}`)
  }
  return value
}

interface Status {
  resource: string
  role: string
  model: string
  documents: number
}

const client = new CommandsClient({
  managerUrl: required("ALIEN_MANAGER_URL"),
  deploymentId: required("ALIEN_DEPLOYMENT_ID"),
  token: required("ALIEN_TOKEN"),
})

// Same command name, two targets, two senders.
const workerStatus = await client.target("api").invoke<Status>("status", {})
const daemonStatus = await client.target("indexer-daemon").invoke<Status>("status", {})

console.log("api            ->", workerStatus)
console.log("indexer-daemon ->", daemonStatus)

// The routing is only meaningful if the two targets answered differently.
if (workerStatus.role === daemonStatus.role) {
  throw new Error(
    `expected the two targets to answer distinctly, but both returned role="${workerStatus.role}"`,
  )
}

// A search reads the shared index the daemon builds; either target can serve it.
const workerHits = await client.target("api").invoke("search", { term: "command" })
console.log("search via api ->", workerHits)

console.log("routing verified: overlapping `status` resolved by target")
