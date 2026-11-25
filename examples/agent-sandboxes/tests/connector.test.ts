import { createServer } from "node:http"
import type { AddressInfo } from "node:net"
import { describe, expect, it } from "vitest"
import { allowed } from "../data-plane/connector/src/access"
import { Approvals } from "../data-plane/connector/src/approvals"
import {
  type ConnectorDeps,
  createConnector,
  isOrdersPath,
  scannerPath,
  TURN_ENDED,
} from "../data-plane/connector/src/handler"
import { scannerGet } from "../data-plane/connector/src/scanner"
import { TurnTokens } from "../data-plane/connector/src/tokens"

describe("turn tokens", () => {
  it("refuses the sandbox's placeholder and a missing header", () => {
    const tokens = new TurnTokens(60_000)
    expect(tokens.check(undefined)).toEqual({ error: "no turn credentials on this request" })
    expect(tokens.check("Bearer placeholder")).toEqual({
      error: "no turn credentials on this request",
    })
  })

  it("maps a token to its thread and user", () => {
    const tokens = new TurnTokens(60_000)
    const { token } = tokens.issue("t1", "alice")
    expect(tokens.check(`Bearer ${token}`)).toMatchObject({ threadId: "t1", user: "alice" })
  })

  it("a new token for the thread ends the previous user's token", () => {
    const tokens = new TurnTokens(60_000)
    const alice = tokens.issue("t1", "alice").token
    const bob = tokens.issue("t1", "bob").token
    expect(tokens.check(`Bearer ${alice}`)).toEqual({ error: "unknown or replaced turn token" })
    expect(tokens.check(`Bearer ${bob}`)).toMatchObject({ user: "bob" })
  })

  it("revoke ends the thread's token, and only that thread's", () => {
    const tokens = new TurnTokens(60_000)
    const a = tokens.issue("t1", "alice").token
    const b = tokens.issue("t2", "alice").token
    expect(tokens.isCurrent(a)).toBe(true)
    expect(tokens.revoke("t1")).toBe(true)
    expect(tokens.isCurrent(a)).toBe(false)
    expect(tokens.check(`Bearer ${a}`)).toEqual({ error: "unknown or replaced turn token" })
    expect(tokens.isCurrent(b)).toBe(true)
    expect(tokens.revoke("t1")).toBe(false)
  })

  it("expires", () => {
    let now = 1_000
    const tokens = new TurnTokens(10, () => now)
    const { token } = tokens.issue("t1", "alice")
    now = 1_010
    expect(tokens.check(`Bearer ${token}`)).toEqual({ error: "turn token expired" })
  })
})

describe("access", () => {
  it("alice writes, bob only reads", () => {
    expect(allowed("alice", "orders:write")).toBe(true)
    expect(allowed("bob", "orders:read")).toBe(true)
    expect(allowed("bob", "orders:write")).toBe(false)
    expect(allowed("carol", "orders:read")).toBe(false)
  })
})

describe("approvals", () => {
  const write = {
    threadId: "t1",
    user: "alice",
    method: "PATCH",
    path: "/orders/1003",
    body: { status: "refunded" },
  }

  it("holds a write until it is approved", async () => {
    const approvals = new Approvals(60_000)
    const { write: held, decision } = approvals.hold(write, "tok")
    expect(approvals.list("t1").map(w => w.id)).toEqual([held.id])
    expect(approvals.list("t2")).toEqual([])
    expect(approvals.decide(held.id, { approved: true, by: "alice" })).toBe(true)
    await expect(decision).resolves.toEqual({ approved: true, by: "alice" })
    expect(approvals.list()).toEqual([])
    expect(approvals.decide(held.id, { approved: true, by: "alice" })).toBe(false)
  })

  it("a write nobody decides is denied at the timeout", async () => {
    const approvals = new Approvals(5)
    const { decision } = approvals.hold(write, "tok")
    await expect(decision).resolves.toMatchObject({ approved: false, by: "timeout" })
    expect(approvals.list()).toEqual([])
  })
})

describe("scanner", () => {
  it("routes only /scanner/* paths", () => {
    expect(scannerPath("/scanner/findings")).toBe("/findings")
    expect(scannerPath("/orders")).toBeUndefined()
    expect(scannerPath("/scanner")).toBeUndefined()
  })

  it("every user may read it, an unknown user may not", () => {
    expect(allowed("bob", "scanner:read")).toBe(true)
    expect(allowed("carol", "scanner:read")).toBe(false)
  })

  it("adds the customer's key; the caller never supplies one", async () => {
    const seen: Array<string | undefined> = []
    const server = createServer((req, res) => {
      seen.push(req.headers.authorization)
      res.end(JSON.stringify({ findings: [] }))
    })
    await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve))
    const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
    try {
      expect(await scannerGet(url, "customer-key", "/findings")).toEqual({
        status: 200,
        body: { findings: [] },
      })
      expect(seen).toEqual(["Bearer customer-key"])
      expect((await scannerGet(url, "", "/findings")).status).toBe(503)
    } finally {
      server.close()
    }
  })
})

describe("orders paths", () => {
  it("matches /orders and /orders/..., nothing that only starts with the letters", () => {
    expect(isOrdersPath("/orders")).toBe(true)
    expect(isOrdersPath("/orders/1001")).toBe(true)
    expect(isOrdersPath("/orders-admin")).toBe(false)
    expect(isOrdersPath("/ordersx/1")).toBe(false)
  })
})

// The connector's HTTP handler on a real server, with a fake internal API behind it.
async function startConnector() {
  const tokens = new TurnTokens(60_000)
  const approvals = new Approvals(60_000)
  const upstream: Array<{ method: string; path: string; body?: string }> = []
  const deps: ConnectorDeps = {
    tokens,
    approvals,
    internal: async (method, path, body) => {
      upstream.push({ method, path, body })
      return { status: 200, body: { ok: true } }
    },
    scanner: async () => ({ status: 200, body: { findings: [] } }),
  }
  const connector = createConnector(deps)
  const server = createServer((req, res) => void connector.handle(req, res))
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve))
  const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
  const call = (path: string, token: string | undefined, init: RequestInit = {}) =>
    fetch(base + path, { ...init, headers: token ? { authorization: `Bearer ${token}` } : {} })
  const write = (path: string, token: string) =>
    call(path, token, { method: "PATCH", body: JSON.stringify({ status: "refunded" }) })
  const held = async () => {
    for (let i = 0; i < 200 && approvals.list().length === 0; i++)
      await new Promise(r => setTimeout(r, 5))
    return approvals.list()[0]
  }
  return { tokens, approvals, connector, upstream, call, write, held, close: () => server.close() }
}

describe("connector requests", () => {
  it("checks the token before routing: no token is a 401 even on a path that does not exist", async () => {
    const c = await startConnector()
    try {
      expect((await c.call("/nothing", undefined)).status).toBe(401)
      expect((await c.call("/orders", "placeholder")).status).toBe(401)
      expect((await c.call("/orders", "not-a-token")).status).toBe(401)
      const { token } = c.tokens.issue("t1", "alice")
      expect((await c.call("/nothing", token)).status).toBe(404)
      expect((await c.call("/orders-admin", token)).status).toBe(404)
      expect((await c.call("/orders", token)).status).toBe(200)
      expect(c.upstream).toEqual([{ method: "GET", path: "/orders", body: undefined }])
    } finally {
      c.close()
    }
  })

  it("answers bob's write with a 403 and holds nothing", async () => {
    const c = await startConnector()
    try {
      const { token } = c.tokens.issue("t1", "bob")
      const response = await c.write("/orders/1004", token)
      expect(response.status).toBe(403)
      expect(c.approvals.list()).toEqual([])
      expect(c.upstream).toEqual([])
    } finally {
      c.close()
    }
  })

  it("holds alice's write and forwards it only after it is approved", async () => {
    const c = await startConnector()
    try {
      const { token } = c.tokens.issue("t1", "alice")
      const response = c.write("/orders/1003", token)
      const write = await c.held()
      expect(write).toMatchObject({ threadId: "t1", user: "alice", path: "/orders/1003" })
      expect(JSON.stringify(c.approvals.list())).not.toContain(token)
      await new Promise(r => setTimeout(r, 30))
      expect(c.upstream).toEqual([])
      expect(
        c.connector.decide({ id: write.id, threadId: "t1", approve: true, by: "manager" }),
      ).toEqual({ decided: true })
      const done = await response
      expect(done.status).toBe(200)
      expect(await done.json()).toMatchObject({ approvedBy: "manager" })
      expect(c.upstream.map(u => `${u.method} ${u.path}`)).toEqual(["PATCH /orders/1003"])
    } finally {
      c.close()
    }
  })

  it("a denied write is never forwarded", async () => {
    const c = await startConnector()
    try {
      const { token } = c.tokens.issue("t1", "alice")
      const response = c.write("/orders/1003", token)
      const write = await c.held()
      c.connector.decide({ id: write.id, threadId: "t1", approve: false, by: "manager" })
      expect((await response).status).toBe(403)
      expect(c.upstream).toEqual([])
    } finally {
      c.close()
    }
  })

  it("an approval after a user switch is refused and the write is not forwarded", async () => {
    const c = await startConnector()
    try {
      const alice = c.tokens.issue("t1", "alice").token
      const response = c.write("/orders/1003", alice)
      const write = await c.held()
      c.tokens.issue("t1", "bob") // the thread now belongs to bob
      expect(
        c.connector.decide({ id: write.id, threadId: "t1", approve: true, by: "manager" }),
      ).toEqual({
        decided: false,
        reason: TURN_ENDED,
      })
      const done = await response
      expect(done.status).toBe(403)
      expect(JSON.stringify(await done.json())).toContain(TURN_ENDED)
      expect(c.upstream).toEqual([])
    } finally {
      c.close()
    }
  })

  it("an approval after the turn ended (token revoked) is refused", async () => {
    const c = await startConnector()
    try {
      const { token } = c.tokens.issue("t1", "alice")
      const response = c.write("/orders/1003", token)
      const write = await c.held()
      c.tokens.revoke("t1")
      expect(
        c.connector.decide({ id: write.id, threadId: "t1", approve: true, by: "manager" }).decided,
      ).toBe(false)
      expect((await response).status).toBe(403)
      expect(c.upstream).toEqual([])
    } finally {
      c.close()
    }
  })

  it("a token that ends while the decision is being made still stops the forward", async () => {
    const c = await startConnector()
    try {
      const { token } = c.tokens.issue("t1", "alice")
      const response = c.write("/orders/1003", token)
      const write = await c.held()
      // Decided, then the turn ends before the connector forwards.
      c.approvals.decide(write.id, { approved: true, by: "manager" })
      c.tokens.revoke("t1")
      const done = await response
      expect(done.status).toBe(409)
      expect(JSON.stringify(await done.json())).toContain(TURN_ENDED)
      expect(c.upstream).toEqual([])
    } finally {
      c.close()
    }
  })
})

describe("who may approve", () => {
  async function held() {
    const c = await startConnector()
    const { token } = c.tokens.issue("t1", "alice")
    const response = c.write("/orders/1003", token)
    const write = await c.held()
    return { c, write, response }
  }

  it("not the user who asked, not a user without approve rights, not an unknown user", async () => {
    const { c, write, response } = await held()
    try {
      const decide = (by: string) =>
        c.connector.decide({ id: write.id, threadId: "t1", approve: true, by })
      expect(decide("alice").decided).toBe(false) // asked for it, and has no approve right
      expect(decide("bob").reason).toBe("bob may not approve writes")
      expect(decide("carol").reason).toBe("carol may not approve writes")
      expect(c.approvals.list()).toHaveLength(1)
      expect(decide("manager").decided).toBe(true)
      expect((await response).status).toBe(200)
    } finally {
      c.close()
    }
  })

  it("an approver who also asked is refused as the requester", async () => {
    const c = await startConnector()
    try {
      // A user with both rights would still not approve their own write.
      const { token } = c.tokens.issue("t1", "manager")
      const approvalsOf = c.approvals
      const { write } = approvalsOf.hold(
        { threadId: "t1", user: "manager", method: "PATCH", path: "/orders/1", body: {} },
        token,
      )
      expect(
        c.connector.decide({ id: write.id, threadId: "t1", approve: true, by: "manager" }).reason,
      ).toBe("the user who asked cannot approve their own write")
    } finally {
      c.close()
    }
  })

  it("a decision under the wrong thread is refused", async () => {
    const { c, write } = await held()
    try {
      expect(
        c.connector.decide({ id: write.id, threadId: "t2", approve: true, by: "manager" }),
      ).toEqual({
        decided: false,
        reason: "that write belongs to another thread",
      })
      expect(c.approvals.list()).toHaveLength(1)
    } finally {
      c.close()
    }
  })

  it("a write that is gone is reported as gone", async () => {
    const c = await startConnector()
    try {
      expect(
        c.connector.decide({ id: "nope", threadId: "t1", approve: true, by: "manager" }).decided,
      ).toBe(false)
    } finally {
      c.close()
    }
  })
})
