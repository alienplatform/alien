/**
 * Comprehensive TypeScript Test App
 *
 * Tests all TypeScript SDK features: bindings, commands, events, SSE, background tasks.
 * Exposes HTTP endpoints that the e2e check functions exercise.
 */

import { createHash } from "node:crypto"
import { command, kv, onCronEvent, onQueueMessage, onStorageEvent } from "@alienplatform/sdk"
import { Hono } from "hono"

import { sanitizeKvKeyPart } from "./helpers.js"

import environmentRoutes from "./handlers/environment.js"
import eventsRoutes from "./handlers/events.js"
import healthRoutes from "./handlers/health.js"
import inspectRoutes from "./handlers/inspect.js"
import kvRoutes from "./handlers/kv.js"
import postgresRoutes from "./handlers/postgres.js"
import queueRoutes from "./handlers/queue.js"
import sseRoutes from "./handlers/sse.js"
import storageRoutes from "./handlers/storage.js"
import vaultRoutes from "./handlers/vault.js"
import waitUntilRoutes from "./handlers/wait-until.js"

const app = new Hono()

// Mount handler routes
app.route("/", healthRoutes)
app.route("/", environmentRoutes)
app.route("/", inspectRoutes)
app.route("/", sseRoutes)
app.route("/", storageRoutes)
app.route("/", kvRoutes)
app.route("/", vaultRoutes)
app.route("/", postgresRoutes)
app.route("/", queueRoutes)
app.route("/", eventsRoutes)
app.route("/", waitUntilRoutes)

// --- Event handlers ---

onStorageEvent("*", async event => {
  const k = kv("alien-kv")
  const sanitizedKey = sanitizeKvKeyPart(event.objectKey)
  await k.setJson(`storage_event:${sanitizedKey}`, {
    key: event.objectKey,
    bucket: event.bucketName,
    eventType: event.eventType,
    size: event.size,
    processedAt: new Date().toISOString(),
  })
})

onCronEvent("*", async event => {
  const k = kv("alien-kv")
  const sanitizedSchedule = sanitizeKvKeyPart(event.scheduleName)
  await k.setJson(`cron_event:${sanitizedSchedule}`, {
    scheduleName: event.scheduleName,
    scheduledTime: event.timestamp,
    processedAt: new Date().toISOString(),
  })
})

onQueueMessage("*", async message => {
  const k = kv("alien-kv")
  const sanitizedId = sanitizeKvKeyPart(message.id)
  // The SDK delivers the payload already decoded: a parsed JSON value when
  // the message body is JSON, otherwise the UTF-8 text. Re-serialize objects
  // so the recorded payload stays a string the checks can match against.
  await k.setJson(`queue_message:${sanitizedId}`, {
    messageId: message.id,
    source: message.source,
    payload:
      typeof message.payload === "string" ? message.payload : JSON.stringify(message.payload),
    processedAt: new Date().toISOString(),
  })
})

// --- Commands ---

command("echo", async (params: any) => {
  return params
})

command("cmd-test-small", async (params: any) => {
  const paramsJson = JSON.stringify(params)
  const hash = createHash("sha256").update(paramsJson).digest("hex")

  return {
    success: true,
    testType: "cmd-small-payload",
    paramsHash: hash,
    paramsSize: paramsJson.length,
    timestamp: new Date().toISOString(),
    message: "Small payload test completed successfully",
  }
})

command("cmd-test-large-response", async (params: any) => {
  const paramsJson = JSON.stringify(params)
  const hash = createHash("sha256").update(paramsJson).digest("hex")

  const bulkArray = Array.from({ length: 8000 }, (_, i) => `bulk-item-${i}`)
  const largeData = Array.from({ length: 15000 }, () => "test-data-chunk").join(" ")

  return {
    success: true,
    testType: "cmd-large-payload",
    paramsHash: hash,
    paramsSize: paramsJson.length,
    timestamp: new Date().toISOString(),
    message: "Large response test completed successfully",
    largeResponseData: largeData,
    bulkData: bulkArray,
  }
})

// Medium request (~50KB), small response
// Tests auto-promote + re-inline path (>20KB KV limit, <150KB transport limit)
command("cmd-test-medium-request", async (params: any) => {
  const paramsJson = JSON.stringify(params)
  const hash = createHash("sha256").update(paramsJson).digest("hex")

  return {
    success: true,
    testType: "cmd-medium-request",
    paramsHash: hash,
    paramsSize: paramsJson.length,
    timestamp: new Date().toISOString(),
    message: "Medium request test completed successfully",
  }
})

command("cmd-test-large-request", async (params: any) => {
  const paramsJson = JSON.stringify(params)
  const hash = createHash("sha256").update(paramsJson).digest("hex")

  return {
    success: true,
    testType: "cmd-large-request",
    paramsHash: hash,
    paramsSize: paramsJson.length,
    timestamp: new Date().toISOString(),
    message: "Large request test completed successfully",
  }
})

command("cmd-test-large-both", async (params: any) => {
  const paramsJson = JSON.stringify(params)
  const hash = createHash("sha256").update(paramsJson).digest("hex")

  const bulkArray = Array.from({ length: 8000 }, (_, i) => `bulk-item-${i}`)
  const largeData = Array.from({ length: 15000 }, () => "test-data-chunk").join(" ")

  return {
    success: true,
    testType: "cmd-large-both",
    paramsHash: hash,
    paramsSize: paramsJson.length,
    timestamp: new Date().toISOString(),
    message: "Large both test completed successfully",
    largeResponseData: largeData,
    bulkData: bulkArray,
  }
})

export default app
