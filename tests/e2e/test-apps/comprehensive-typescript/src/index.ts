/**
 * Comprehensive TypeScript Test App
 *
 * Tests all TypeScript SDK features: bindings, commands, events, SSE, background tasks.
 * Exposes HTTP endpoints that the e2e check functions exercise.
 */

import { Hono } from "hono"
import { AlienError } from "@aliendotdev/core"
import {
  storage,
  kv,
  queue,
  vault,
  onStorageEvent,
  onQueueMessage,
  command,
  waitUntil,
} from "@aliendotdev/bindings"

const app = new Hono()

async function toExternalOperationError(error: unknown, operation: string) {
  const source = await AlienError.from(error)
  return new AlienError({
    code: "E2E_OPERATION_FAILED",
    message: `Operation '${operation}' failed`,
    retryable: source.retryable,
    internal: false,
    httpStatusCode: 500,
    context: { operation },
    source: source.toOptions(),
  }).toExternal()
}

// --- Health and utility endpoints ---

app.get("/health", (c) => {
  return c.json({ status: "ok", timestamp: new Date().toISOString() })
})

app.get("/hello", (c) => {
  return c.json({ message: "Hello from TypeScript!", timestamp: new Date().toISOString() })
})

app.get("/env-var/:varName", (c) => {
  const varName = c.req.param("varName")
  const value = process.env[varName]
  if (!value) {
    return c.json({ error: `Environment variable ${varName} not found` }, 404)
  }
  return c.json({ name: varName, value })
})

app.post("/inspect", async (c) => {
  const body = await c.req.json()
  return c.json({ success: true, requestBody: body })
})

// --- SSE endpoint ---

app.get("/sse", (c) => {
  const encoder = new TextEncoder()
  const stream = new ReadableStream({
    start(controller) {
      for (let i = 0; i < 10; i++) {
        controller.enqueue(encoder.encode(`data: sse_message_${i}\n\n`))
      }
      controller.close()
    },
  })
  return new Response(stream, {
    headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" },
  })
})

// --- Storage binding test ---

app.post("/storage-test/:bindingName", async (c) => {
  const bindingName = c.req.param("bindingName")
  try {
    const s = await storage(bindingName)
    const testKey = `test-${Date.now()}.txt`
    const content = "test content from e2e"

    await s.put(testKey, content)
    const retrieved = await s.get(testKey)
    const retrievedContent = new TextDecoder().decode(retrieved.data)
    let listCount = 0
    for await (const _ of s.list("test-")) {
      listCount++
    }
    await s.delete(testKey)

    return c.json({
      success: true,
      bindingName,
      operations: {
        put: { key: testKey, success: true },
        get: { content: retrievedContent, success: retrievedContent === content },
        list: { count: listCount, success: true },
        delete: { success: true },
      },
    })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "storage-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

// --- KV binding test ---

app.post("/kv-test/:bindingName", async (c) => {
  const bindingName = c.req.param("bindingName")
  try {
    const k = await kv(bindingName)
    const testKey = `test-key-${Date.now()}`
    const testValue = { message: "kv-test", ts: Date.now() }

    await k.set(testKey, testValue)
    const retrieved = await k.get(testKey)
    const value = retrieved ? JSON.parse(new TextDecoder().decode(retrieved)) : null
    await k.delete(testKey)

    return c.json({
      success: true,
      bindingName,
      operations: {
        set: { key: testKey, success: true },
        get: { value, success: true },
        delete: { success: true },
      },
    })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "kv-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

// --- Vault binding test ---

app.post("/vault-test/:bindingName", async (c) => {
  const bindingName = c.req.param("bindingName")
  try {
    const v = await vault(bindingName)
    const testKey = `test-secret-${Date.now()}`
    const testValue = "test-secret-value"

    await v.set(testKey, testValue)
    const retrieved = await v.get(testKey)
    await v.delete(testKey)

    return c.json({
      success: true,
      bindingName,
      operations: {
        set: { key: testKey, success: true },
        get: { value: retrieved, success: retrieved === testValue },
        delete: { success: true },
      },
    })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "vault-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

// --- Queue binding test ---

app.post("/queue-test/:bindingName", async (c) => {
  const bindingName = c.req.param("bindingName")
  try {
    const q = await queue(bindingName)
    await q.send("default", { test: true, ts: Date.now() })

    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "queue-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

// --- External secret endpoint ---

app.get("/external-secret", async (c) => {
  try {
    const v = await vault("test-alien-vault")
    const value = await v.get("EXTERNAL_TEST_SECRET")
    return c.json({ exists: !!value, value })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "external-secret")
    return c.json({ exists: false, error: alienError.message, code: alienError.code })
  }
})

// --- Wait-until background task ---

app.post("/wait-until-test", async (c) => {
  const { storageBindingName, testData, delayMs } = await c.req.json()
  const testId = `test-${Date.now()}`

  waitUntil((async () => {
    await new Promise((resolve) => setTimeout(resolve, delayMs || 1000))
    const s = await storage(storageBindingName || "test-alien-storage")
    await s.put(`wait-until-${testId}.txt`, testData || "background-task-done")
  })())

  return c.json({ success: true, testId, message: "Background task scheduled" })
})

app.get("/wait-until-verify/:testId/:storageBindingName", async (c) => {
  const testId = c.req.param("testId")
  const storageBindingName = c.req.param("storageBindingName")
  try {
    const s = await storage(storageBindingName)
    const exists = await s.exists(`wait-until-${testId}.txt`)
    if (!exists) {
      return c.json({
        success: false,
        testId,
        backgroundTaskCompleted: false,
        message: "File not found yet",
      })
    }
    const result = await s.get(`wait-until-${testId}.txt`)
    const fileContent = new TextDecoder().decode(result.data)
    return c.json({
      success: true,
      testId,
      backgroundTaskCompleted: true,
      fileContent,
      message: "Background task completed",
    })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "wait-until-verify")
    return c.json({
      success: false,
      testId,
      backgroundTaskCompleted: false,
      message: alienError.message,
      code: alienError.code,
    })
  }
})

// --- Event verification endpoints ---

app.get("/events/list", async (c) => {
  return c.json({ storageEvents: [], cronEvents: [], queueMessages: [] })
})

app.get("/events/storage/:key", async (c) => {
  const key = c.req.param("key")
  try {
    const k = await kv("test-alien-kv")
    const sanitizedKey = key.replace(/\//g, "_")
    const data = await k.get(`storage_event:${sanitizedKey}`)
    if (!data) return c.json({ found: false })
    return c.json({ found: true, event: JSON.parse(new TextDecoder().decode(data)) })
  } catch {
    return c.json({ found: false })
  }
})

app.get("/events/queue/:messageId", async (c) => {
  const messageId = c.req.param("messageId")
  try {
    const k = await kv("test-alien-kv")
    const sanitizedId = messageId.replace(/\//g, "_")
    const data = await k.get(`queue_message:${sanitizedId}`)
    if (!data) return c.json({ found: false })
    return c.json({ found: true, event: JSON.parse(new TextDecoder().decode(data)) })
  } catch {
    return c.json({ found: false })
  }
})

// --- Event handlers ---

onStorageEvent("*", async (event) => {
  const k = await kv("test-alien-kv")
  const sanitizedKey = event.key.replace(/\//g, "_")
  await k.set(
    `storage_event:${sanitizedKey}`,
    {
      key: event.key,
      bucket: event.bucket,
      eventType: event.eventType,
      size: event.size,
      processedAt: new Date().toISOString(),
    }
  )
})

onQueueMessage("*", async (message) => {
  const k = await kv("test-alien-kv")
  const sanitizedId = message.id.replace(/\//g, "_")
  await k.set(
    `queue_message:${sanitizedId}`,
    {
      messageId: message.id,
      source: message.source,
      payload: typeof message.payload === "string" ? message.payload : new TextDecoder().decode(message.payload as Uint8Array),
      processedAt: new Date().toISOString(),
    }
  )
})

// --- ARC Commands ---

command("echo", async (params: any) => {
  return params
})

command("arc-test-small", async (params: any) => {
  const paramsJson = JSON.stringify(params)
  const crypto = await import("node:crypto")
  const hash = crypto.createHash("sha256").update(paramsJson).digest("hex")

  return {
    success: true,
    testType: "arc-small-payload",
    paramsHash: hash,
    paramsSize: paramsJson.length,
    timestamp: new Date().toISOString(),
  }
})

command("arc-test-large", async (params: any) => {
  const paramsJson = JSON.stringify(params)
  const crypto = await import("node:crypto")
  const hash = crypto.createHash("sha256").update(paramsJson).digest("hex")

  const bulkArray = Array.from({ length: 1500 }, (_, i) => `bulk-item-${i}`)

  return {
    success: true,
    testType: "arc-large-payload",
    paramsHash: hash,
    paramsSize: paramsJson.length,
    timestamp: new Date().toISOString(),
    bulkData: bulkArray,
  }
})

export default app
