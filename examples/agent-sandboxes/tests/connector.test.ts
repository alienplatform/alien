import { createServer } from "node:http"
import type { AddressInfo } from "node:net"
import { describe, expect, it } from "vitest"
import { allowed } from "../data-plane/connector/src/access"
import { Approvals } from "../data-plane/connector/src/approvals"
import { scannerGet, scannerPath } from "../data-plane/connector/src/scanner"
import { TurnTokens } from "../data-plane/connector/src/tokens"

describe("turn tokens", () => {
  it("refuses the sandbox's placeholder and a missing header", () => {
    const tokens = new TurnTokens(60_000)
    expect(tokens.check(undefined)).toEqual({ error: "no turn credentials on this request" })
    expect(tokens.check("Bearer placeholder")).toEqual({ error: "no turn credentials on this request" })
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
    expect(allowed("mallory", "orders:read")).toBe(false)
  })
})

describe("approvals", () => {
  const write = { threadId: "t1", user: "alice", method: "PATCH", path: "/orders/1003", body: { status: "refunded" } }

  it("holds a write until it is approved", async () => {
    const approvals = new Approvals(60_000)
    const { write: held, decision } = approvals.hold(write)
    expect(approvals.list("t1").map(w => w.id)).toEqual([held.id])
    expect(approvals.list("t2")).toEqual([])
    expect(approvals.decide(held.id, { approved: true, by: "alice" })).toBe(true)
    await expect(decision).resolves.toEqual({ approved: true, by: "alice" })
    expect(approvals.list()).toEqual([])
    expect(approvals.decide(held.id, { approved: true, by: "alice" })).toBe(false)
  })

  it("a write nobody decides is denied at the timeout", async () => {
    const approvals = new Approvals(5)
    const { decision } = approvals.hold(write)
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
    expect(allowed("mallory", "scanner:read")).toBe(false)
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
      expect(await scannerGet(url, "customer-key", "/findings")).toEqual({ status: 200, body: { findings: [] } })
      expect(seen).toEqual(["Bearer customer-key"])
      expect((await scannerGet(url, "", "/findings")).status).toBe(503)
    } finally {
      server.close()
    }
  })
})
