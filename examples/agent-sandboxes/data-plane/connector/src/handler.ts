// The connector's HTTP side and its approval rules, apart from the process wiring in index.ts
// so they can be exercised with a plain HTTP server in the tests.

import type { IncomingMessage, ServerResponse } from "node:http"
import { allowed } from "./access"
import type { Approvals } from "./approvals"
import type { TurnTokens } from "./tokens"

export const TURN_ENDED = "the request's turn ended; ask again"

export interface Upstream {
  status: number
  body: unknown
}

export interface ConnectorDeps {
  tokens: TurnTokens
  approvals: Approvals
  /** The customer's internal API. */
  internal(method: string, path: string, body?: string): Promise<Upstream>
  /** The scanner, with the customer's key added. `path` includes the query. */
  scanner(path: string): Promise<Upstream>
  log?: (line: string) => void
}

export interface Decision {
  id: string
  threadId: string
  approve: boolean
  by: string
}

/** `/orders` and `/orders/...`, nothing that merely starts with those letters. */
export function isOrdersPath(pathname: string): boolean {
  return pathname === "/orders" || pathname.startsWith("/orders/")
}

/** `/scanner/findings` is the scanner's `/findings`. */
export function scannerPath(pathname: string): string | undefined {
  return pathname.startsWith("/scanner/") ? pathname.slice("/scanner".length) : undefined
}

function send(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { "content-type": "application/json" })
  res.end(JSON.stringify(body))
}

async function readBody(req: IncomingMessage): Promise<string> {
  let raw = ""
  for await (const chunk of req) raw += chunk
  return raw
}

export function createConnector(deps: ConnectorDeps) {
  const { tokens, approvals } = deps
  const log = deps.log ?? (() => {})

  async function handle(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const url = new URL(req.url ?? "/", "http://connector")
    if (url.pathname === "/health") return send(res, 200, { ok: true })

    // Who is asking comes first: an unknown path gets no answer before the token is checked.
    const grant = tokens.check(req.headers.authorization)
    if ("error" in grant) {
      log(`refused ${req.method} ${url.pathname}: ${grant.error}`)
      return send(res, 401, { error: grant.error })
    }
    const { user, threadId } = grant

    const scanner = scannerPath(url.pathname)
    if (scanner) {
      if (req.method !== "GET") return send(res, 405, { error: "the scanner is read only" })
      if (!allowed(user, "scanner:read"))
        return send(res, 403, { error: `${user} may not read scanner findings` })
      const result = await deps.scanner(scanner + url.search)
      log(`${user} read scanner ${scanner}: ${result.status}`)
      return send(res, result.status, result.body)
    }
    if (!isOrdersPath(url.pathname)) return send(res, 404, { error: "not found" })

    if (req.method === "GET") {
      if (!allowed(user, "orders:read"))
        return send(res, 403, { error: `${user} may not read orders` })
      const result = await deps.internal("GET", url.pathname)
      log(`${user} read ${url.pathname}: ${result.status}`)
      return send(res, result.status, result.body)
    }

    if (req.method === "PATCH" && url.pathname !== "/orders") {
      if (!allowed(user, "orders:write")) {
        log(`refused ${user} write ${url.pathname}: no write access`)
        return send(res, 403, { error: `${user} may not change orders` })
      }
      const raw = await readBody(req)
      let body: unknown
      try {
        body = JSON.parse(raw)
      } catch {
        return send(res, 400, { error: "body must be JSON" })
      }
      const { write, decision } = approvals.hold(
        { threadId, user, method: "PATCH", path: url.pathname, body },
        grant.token,
      )
      log(`holding ${user} write ${url.pathname} as ${write.id}`)
      const outcome = await decision
      if (!outcome.approved) {
        log(`denied ${write.id} by ${outcome.by}`)
        return send(res, 403, { error: `write denied by ${outcome.by}: ${outcome.reason}` })
      }
      // The approval came some time after the request. The request goes on only if the token
      // that asked is still the thread's live token: the user may have changed, or the turn ended.
      if (!tokens.isCurrent(grant.token)) {
        log(`refused ${write.id}: ${TURN_ENDED}`)
        return send(res, 409, { error: TURN_ENDED })
      }
      const result = await deps.internal("PATCH", url.pathname, raw)
      log(`approved ${write.id} by ${outcome.by}: ${result.status}`)
      return send(res, result.status, { approvedBy: outcome.by, result: result.body })
    }

    send(res, 405, { error: "method not allowed" })
  }

  /**
   * One approval decision, from the control plane. The control plane has no sign-in of its own
   * in this example, so `by` is a claim; what the connector enforces is that the approver is a
   * known user with approve rights, is not the user who asked, and that the write belongs to
   * the thread the decision names and to a turn that is still live.
   */
  function decide(input: Decision): { decided: boolean; reason?: string } {
    const held = approvals.get(input.id)
    if (!held) return { decided: false, reason: "that write is no longer waiting" }
    if (held.write.threadId !== input.threadId)
      return { decided: false, reason: "that write belongs to another thread" }
    if (!allowed(input.by, "orders:approve"))
      return { decided: false, reason: `${input.by} may not approve writes` }
    if (input.by === held.write.user) {
      return { decided: false, reason: "the user who asked cannot approve their own write" }
    }
    if (!tokens.isCurrent(held.token)) {
      approvals.decide(input.id, { approved: false, by: "connector", reason: TURN_ENDED })
      return { decided: false, reason: TURN_ENDED }
    }
    const decided = approvals.decide(
      input.id,
      input.approve
        ? { approved: true, by: input.by }
        : { approved: false, by: input.by, reason: "denied in the control plane" },
    )
    return { decided }
  }

  return { handle, decide }
}
