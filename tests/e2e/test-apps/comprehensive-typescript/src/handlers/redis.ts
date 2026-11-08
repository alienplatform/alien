import { redis } from "@alienplatform/sdk"
import { Queue, Worker } from "bullmq"
import { Hono } from "hono"
import { Redis } from "ioredis"
import { createClient } from "redis"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

// Resolves the binding, then drives the server with stock clients: ioredis for the command set a
// real workload uses (TTL, large values, multi-key Lua, transactions, streams with a consumer
// group, pub/sub, blocking pops, leases), node-redis for a basic round trip, and BullMQ for a
// job. The binding is connection-only, so proving it works means speaking the wire protocol.
app.post("/redis-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  const suffix = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  try {
    const conn = await redis(bindingName).connection()
    const options = {
      host: conn.host,
      port: conn.port,
      username: conn.username,
      password: conn.password,
      tls: conn.tls || undefined,
    }

    const client = new Redis({ ...options, maxRetriesPerRequest: 3 })
    const subscriber = new Redis(options)
    try {
      const failures: string[] = []
      const check = (name: string, ok: boolean, detail?: unknown) => {
        if (!ok) failures.push(`${name}: ${JSON.stringify(detail)}`)
      }

      // SET with a TTL, then GET.
      const k = (name: string) => `redis-test:${suffix}:${name}`
      await client.set(k("ttl"), "v", "EX", 60)
      check("get", (await client.get(k("ttl"))) === "v")
      check("ttl", (await client.ttl(k("ttl"))) > 0)

      // A 5 MB value.
      const big = "x".repeat(5 * 1024 * 1024)
      await client.set(k("big"), big)
      check("big value", (await client.get(k("big")))?.length === big.length)

      // Multi-key Lua writing three keys, with key names passed both as KEYS and in ARGV.
      await client.eval(
        "redis.call('SET', KEYS[1], ARGV[1]); redis.call('SET', KEYS[2], ARGV[2]); redis.call('SET', ARGV[4], ARGV[3]); return 1",
        2,
        k("l1"),
        k("l2"),
        "a",
        "b",
        "c",
        k("l3"),
      )
      check("lua", (await client.mget(k("l1"), k("l2"), k("l3"))).join("") === "abc")

      // MULTI/EXEC.
      const tx = await client.multi().set(k("m1"), "1").incr(k("m1")).get(k("m1")).exec()
      check("multi", tx?.[2]?.[1] === "2", tx)

      // Streams with a consumer group.
      const stream = k("stream")
      await client.xgroup("CREATE", stream, "g", "$", "MKSTREAM")
      const id = await client.xadd(stream, "*", "event", "one")
      const read = (await client.xreadgroup(
        "GROUP",
        "g",
        "consumer",
        "COUNT",
        1,
        "STREAMS",
        stream,
        ">",
      )) as Array<[string, Array<[string, string[]]>]> | null
      check("xreadgroup", read?.[0]?.[1]?.[0]?.[0] === id, read)
      check("xack", (await client.xack(stream, "g", id as string)) === 1)

      // Pub/sub round trip.
      const channel = k("channel")
      const received = new Promise<string>(resolve => {
        subscriber.on("message", (_channel, message) => resolve(message))
      })
      await subscriber.subscribe(channel)
      await client.publish(channel, "ping")
      check("pubsub", (await Promise.race([received, timeout(5000)])) === "ping")

      // Blocking pop, lease with SET NX PX, TIME, SCAN, CLIENT LIST.
      await client.rpush(k("list"), "item")
      check("blpop", (await client.blpop(k("list"), 2))?.[1] === "item")
      check("lease", (await client.set(k("lease"), "1", "PX", 5000, "NX")) === "OK")
      check("lease held", (await client.set(k("lease"), "2", "PX", 5000, "NX")) === null)
      check("time", (await client.time()).length === 2)
      const [, scanned] = await client.scan(0, "MATCH", `redis-test:${suffix}:*`, "COUNT", 1000)
      check("scan", Array.isArray(scanned))
      check("client list", String(await client.client("LIST")).includes("addr="))

      // node-redis against the same server.
      const nodeRedis = createClient({
        socket: {
          host: conn.host,
          port: conn.port,
          ...(conn.tls ? { tls: true as const, ...conn.tls } : {}),
        },
        username: conn.username,
        password: conn.password,
      })
      await nodeRedis.connect()
      try {
        await nodeRedis.set(k("node"), "node-redis")
        check("node-redis", (await nodeRedis.get(k("node"))) === "node-redis")
      } finally {
        await nodeRedis.quit()
      }

      // BullMQ job round trip (BullMQ requires maxRetriesPerRequest: null).
      const queueName = `redis-test-${suffix}`
      const bullOptions = { ...options, maxRetriesPerRequest: null as null }
      const queue = new Queue(queueName, { connection: bullOptions })
      let bullWorker: Worker | undefined
      const done = new Promise<string>((resolve, reject) => {
        bullWorker = new Worker(
          queueName,
          async job => {
            resolve(String(job.data.value))
            return "ok"
          },
          { connection: bullOptions },
        )
        bullWorker.on("failed", (_job, error) => reject(error))
        setTimeout(() => reject(new Error("BullMQ job was not processed in 10s")), 10_000)
      })
      try {
        await queue.add("job", { value: "bull" })
        check("bullmq", (await done) === "bull")
      } finally {
        await bullWorker?.close().catch(() => {})
        await queue.obliterate({ force: true }).catch(() => {})
        await queue.close()
      }

      // Clean up what this run created.
      const keys = await client.keys(`redis-test:${suffix}:*`)
      if (keys.length > 0) await client.del(...keys)

      if (failures.length > 0) {
        return c.json({ success: false, error: failures.join("; ") }, 500)
      }
    } finally {
      subscriber.disconnect()
      client.disconnect()
    }
    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "redis-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

function timeout(ms: number): Promise<never> {
  return new Promise((_, reject) =>
    setTimeout(() => reject(new Error(`timed out after ${ms}ms`)), ms),
  )
}

export default app
