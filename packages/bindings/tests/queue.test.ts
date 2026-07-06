/**
 * Queue behavioral tests through the REAL napi addon against the local
 * turso-backed provider (`crates/alien-bindings/src/providers/queue/local.rs`).
 * No mocks: every op does real (sqlite-backed) I/O under a fresh temp
 * directory. The default visibility lease is 30s
 * (`alien_bindings::traits::LEASE_SECONDS`), so "hidden while in flight" is
 * asserted without waiting it out; nack is the documented way to make a
 * message immediately redeliverable.
 */

import { randomUUID } from "node:crypto"
import { afterAll, describe, expect, it } from "vitest"
import { queue } from "../src/index.js"
import type { Queue } from "../src/index.js"
import { cleanupTempDirs, localQueueBindingEnv, only } from "./helpers/local-binding-env.js"

function freshQueue(): Queue {
  const name = `queue-${randomUUID()}`
  const { env } = localQueueBindingEnv(name)
  return queue(name, { env })
}

describe("queue (local turso-backed provider)", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  it("send(json) / receive() returns a typed json payload", async () => {
    const q = freshQueue()
    await q.send({ hello: "world", n: 1 })

    const msg = only(await q.receive(1))

    expect(msg.payloadType).toBe("json")
    expect(msg.payloadText).toBeUndefined()
    expect(JSON.parse(msg.payloadJson as string)).toEqual({ hello: "world", n: 1 })
  })

  it("sendText() / receive() returns a typed text payload", async () => {
    const q = freshQueue()
    await q.sendText("plain text message")

    const msg = only(await q.receive(1))

    expect(msg.payloadType).toBe("text")
    expect(msg.payloadJson).toBeUndefined()
    expect(msg.payloadText).toBe("plain text message")
  })

  it("receive respects max and returns messages in FIFO order", async () => {
    const q = freshQueue()
    await q.sendText("first")
    await q.sendText("second")
    await q.sendText("third")

    const batch = await q.receive(2)

    expect(batch.map(m => m.payloadText)).toEqual(["first", "second"])
  })

  it("ack removes the message so a later receive is empty", async () => {
    const q = freshQueue()
    await q.sendText("ack-me")
    const msg = only(await q.receive(1))

    await q.ack(msg.receiptHandle)

    expect(await q.receive(10)).toEqual([])
  })

  it("nack makes the message immediately redeliverable", async () => {
    const q = freshQueue()
    await q.sendText("nack-me")
    const first = only(await q.receive(1))

    // In flight under the default 30s lease: hidden until nacked.
    expect(await q.receive(10)).toEqual([])

    await q.nack(first.receiptHandle)

    const redelivered = only(await q.receive(10))
    expect(redelivered.payloadText).toBe("nack-me")
    expect(redelivered.receiptHandle).not.toBe(first.receiptHandle)
  })

  it("purge empties the queue, including messages currently in flight", async () => {
    const q = freshQueue()
    await q.sendText("m1")
    await q.sendText("m2")
    await q.receive(1) // one message now in flight, one still visible

    await q.purge()

    expect(await q.receive(10)).toEqual([])
  })
})
