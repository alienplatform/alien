import { afterAll, expect, test } from "bun:test"
import app from "./src/handlers/endpoint-continuity.js"

const server = Bun.serve({ port: 0, fetch: app.fetch })
afterAll(() => server.stop(true))

test("one socket delivers paced frames with stable process and connection identity", async () => {
  const state = await fetch(new URL("/endpoint-continuity", server.url)).then(r => r.json())
  const response = await fetch(
    new URL("/endpoint-continuity/stream?count=3&intervalMs=80", server.url),
  )
  expect(response.status).toBe(200)
  const reader = response.body!.getReader()
  const decoder = new TextDecoder()
  let pending = ""
  const frames: { processId: string; connectionId: string; sequence: number; elapsedMs: number }[] =
    []
  const receivedAt: number[] = []
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    pending += decoder.decode(value, { stream: true })
    while (true) {
      const end = pending.indexOf("\n\n")
      if (end === -1) break
      const line = pending.slice(0, end)
      expect(line.startsWith("data: ")).toBe(true)
      frames.push(JSON.parse(line.slice(6)))
      receivedAt.push(performance.now())
      pending = pending.slice(end + 2)
    }
  }
  expect(pending).toBe("")
  expect(frames.map(frame => frame.sequence)).toEqual([0, 1, 2])
  expect(new Set(frames.map(frame => frame.processId))).toEqual(new Set([state.processId]))
  expect(new Set(frames.map(frame => frame.connectionId)).size).toBe(1)
  // A buffered response can carry correct timestamps but arrive all at once.
  expect(receivedAt[2] - receivedAt[0]).toBeGreaterThanOrEqual(100)
  expect(frames[2].elapsedMs - frames[0].elapsedMs).toBeGreaterThanOrEqual(100)
  const another = await fetch(new URL("/endpoint-continuity/stream?count=1", server.url)).then(r =>
    r.text(),
  )
  const frame = JSON.parse(another.trim().slice(6))
  expect(frame.processId).toBe(state.processId)
  expect(frame.connectionId).not.toBe(frames[0].connectionId)
})

test("cancelling a stream clears its pending delay and connection accounting", async () => {
  const response = await app.request("/endpoint-continuity/stream?intervalMs=30000")
  const reader = response.body!.getReader()
  await reader.read()
  expect(await (await app.request("/endpoint-continuity")).json()).toMatchObject({
    activeStreams: 1,
  })
  await reader.cancel()
  expect(await (await app.request("/endpoint-continuity")).json()).toMatchObject({
    activeStreams: 0,
  })
})

test("invalid stream bounds fail before opening a stream", async () => {
  for (const query of [
    "count=0",
    "count=3601",
    "count=1.5",
    "intervalMs=NaN",
    "intervalMs=9",
    "intervalMs=30001",
  ]) {
    expect((await app.request(`/endpoint-continuity/stream?${query}`)).status).toBe(400)
  }
  expect(await (await app.request("/endpoint-continuity")).json()).toMatchObject({
    activeStreams: 0,
  })
})
