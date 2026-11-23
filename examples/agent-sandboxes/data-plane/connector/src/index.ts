// Connector: the only way from a sandbox to the customer's internal API.
//
// HTTP on :8080, reached by sandboxes only through their link (no public endpoint):
//   GET   /orders, GET /orders/:id     reads, passed through
//   PATCH /orders/:id                  writes, held until approved
//   GET   /scanner/*                   the scanner's reads, with the customer's own key added here
// Every request must carry the per-turn token the gateway adds to the link; the sandbox's own
// `Authorization: Bearer placeholder` is refused.
//
// Commands, from the control plane through the deployment's command channel:
//   issue-token  { threadId, user }               a token for the next turn
//   revoke-token { threadId }                     end the thread's token (the turn is over)
//   approvals    { threadId? }                    writes waiting for a decision
//   decide       { id, threadId, approve, by }    approve or deny one

import { createServer, type ServerResponse } from "node:http"
import { createCommandReceiver } from "@alienplatform/commands"
import { Approvals } from "./approvals"
import { createConnector, type Upstream } from "./handler"
import { scannerGet } from "./scanner"
import { TurnTokens } from "./tokens"

const INTERNAL_API_URL = (process.env.INTERNAL_API_URL ?? "").replace(/\/$/, "")
const SCANNER_URL = process.env.SCANNER_URL ?? ""
// The customer's key, from their own secret store. Only this process reads it.
const SCANNER_API_KEY = process.env.SCANNER_API_KEY ?? ""
// A token ends with its turn; this limit only covers a turn nobody ended, so it must outlast the
// runner's longest turn (30 minutes).
const tokens = new TurnTokens(Number(process.env.TURN_TOKEN_TTL_SECONDS ?? 35 * 60) * 1000)
const approvals = new Approvals(Number(process.env.APPROVAL_TIMEOUT_SECONDS ?? 600) * 1000)

async function internal(method: string, path: string, body?: string): Promise<Upstream> {
  if (!INTERNAL_API_URL) return { status: 503, body: { error: "INTERNAL_API_URL is not set" } }
  const response = await fetch(`${INTERNAL_API_URL}${path}`, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body,
  })
  return { status: response.status, body: await response.json() }
}

const connector = createConnector({
  tokens,
  approvals,
  internal,
  scanner: path => scannerGet(SCANNER_URL, SCANNER_API_KEY, path),
  log: line => console.log(line),
})

function send(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { "content-type": "application/json" })
  res.end(JSON.stringify(body))
}

createServer((req, res) => {
  connector.handle(req, res).catch(error => {
    console.error("request failed", error)
    if (!res.headersSent) send(res, 502, { error: "internal API unreachable" })
  })
}).listen(Number(process.env.PORT ?? 8080), "0.0.0.0", () => {
  console.log(`connector listening on ${process.env.PORT ?? 8080}, internal API ${INTERNAL_API_URL}`)
})

function field(input: unknown, name: string): string {
  const value = typeof input === "object" && input !== null ? (input as Record<string, unknown>)[name] : undefined
  if (typeof value !== "string" || value === "") throw new TypeError(`${name} must be a non-empty string`)
  return value
}

// Several commands at once (an events long-poll must not hold up an approval), polled often
// enough for a chat.
const receiver = createCommandReceiver({ maxLeases: 16, pollIntervalMs: 1000 })

receiver.command("issue-token", async input => {
  const threadId = field(input, "threadId")
  const user = field(input, "user")
  return tokens.issue(threadId, user)
})

receiver.command("approvals", async input => {
  const threadId =
    typeof input === "object" && input !== null && "threadId" in input && typeof input.threadId === "string"
      ? input.threadId
      : undefined
  return { pending: approvals.list(threadId) }
})

receiver.command("revoke-token", async input => ({ revoked: tokens.revoke(field(input, "threadId")) }))

receiver.command("decide", async input => {
  const approve = typeof input === "object" && input !== null && "approve" in input && input.approve === true
  return connector.decide({ id: field(input, "id"), threadId: field(input, "threadId"), approve, by: field(input, "by") })
})

// A source-built container compiles to one CommonJS bundle, which forbids top-level await.
async function main(): Promise<void> {
  await receiver.run()
}

void main()
