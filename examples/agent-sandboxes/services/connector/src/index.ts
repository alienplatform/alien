// Connector: the only way from a sandbox to the customer's internal API.
//
// HTTP on :8080, reached by sandboxes only through their link (no public endpoint):
//   GET   /orders, GET /orders/:id     reads, passed through
//   PATCH /orders/:id                  writes, held until approved
// Every request must carry the per-turn token the gateway adds to the link; the sandbox's own
// `Authorization: Bearer placeholder` is refused.
//
// Commands, from the control plane through the deployment's command channel:
//   issue-token { threadId, user }     a token for the next turn
//   approvals   { threadId? }          writes waiting for a decision
//   decide      { id, approve, by }    approve or deny one

import { createServer, type IncomingMessage, type ServerResponse } from "node:http"
import { createCommandReceiver } from "@alienplatform/commands"
import { allowed } from "./access"
import { Approvals } from "./approvals"
import { TurnTokens } from "./tokens"

const INTERNAL_API_URL = (process.env.INTERNAL_API_URL ?? "").replace(/\/$/, "")
const tokens = new TurnTokens(Number(process.env.TURN_TOKEN_TTL_SECONDS ?? 900) * 1000)
const approvals = new Approvals(Number(process.env.APPROVAL_TIMEOUT_SECONDS ?? 600) * 1000)

function send(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { "content-type": "application/json" })
  res.end(JSON.stringify(body))
}

async function readBody(req: IncomingMessage): Promise<string> {
  let raw = ""
  for await (const chunk of req) raw += chunk
  return raw
}

async function internal(method: string, path: string, body?: string): Promise<{ status: number; body: unknown }> {
  if (!INTERNAL_API_URL) return { status: 503, body: { error: "INTERNAL_API_URL is not set" } }
  const response = await fetch(`${INTERNAL_API_URL}${path}`, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body,
  })
  return { status: response.status, body: await response.json() }
}

async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const url = new URL(req.url ?? "/", "http://connector")
  if (url.pathname === "/health") return send(res, 200, { ok: true })
  if (!url.pathname.startsWith("/orders")) return send(res, 404, { error: "not found" })

  const grant = tokens.check(req.headers.authorization)
  if ("error" in grant) {
    console.log(`refused ${req.method} ${url.pathname}: ${grant.error}`)
    return send(res, 401, { error: grant.error })
  }
  const { user, threadId } = grant

  if (req.method === "GET") {
    if (!allowed(user, "orders:read")) return send(res, 403, { error: `${user} may not read orders` })
    const result = await internal("GET", url.pathname)
    console.log(`${user} read ${url.pathname}: ${result.status}`)
    return send(res, result.status, result.body)
  }

  if (req.method === "PATCH") {
    if (!allowed(user, "orders:write")) {
      console.log(`refused ${user} write ${url.pathname}: no write access`)
      return send(res, 403, { error: `${user} may not change orders` })
    }
    const raw = await readBody(req)
    let body: unknown
    try {
      body = JSON.parse(raw)
    } catch {
      return send(res, 400, { error: "body must be JSON" })
    }
    const { write, decision } = approvals.hold({ threadId, user, method: "PATCH", path: url.pathname, body })
    console.log(`holding ${user} write ${url.pathname} as ${write.id}`)
    const outcome = await decision
    if (!outcome.approved) {
      console.log(`denied ${write.id} by ${outcome.by}`)
      return send(res, 403, { error: `write denied by ${outcome.by}: ${outcome.reason}` })
    }
    const result = await internal("PATCH", url.pathname, raw)
    console.log(`approved ${write.id} by ${outcome.by}: ${result.status}`)
    return send(res, result.status, { approvedBy: outcome.by, result: result.body })
  }

  send(res, 405, { error: "method not allowed" })
}

createServer((req, res) => {
  handle(req, res).catch(error => {
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

receiver.command("decide", async input => {
  const id = field(input, "id")
  const by = field(input, "by")
  const approve = typeof input === "object" && input !== null && "approve" in input && input.approve === true
  const decided = approvals.decide(
    id,
    approve ? { approved: true, by } : { approved: false, by, reason: "denied in the control plane" },
  )
  return { decided }
})

// A source-built container compiles to one CommonJS bundle, which forbids top-level await.
async function main(): Promise<void> {
  await receiver.run()
}

void main()
