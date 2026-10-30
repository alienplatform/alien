import { createChannel, createServer } from "nice-grpc"
import { afterEach, expect, it } from "vitest"
import { EventLoop } from "../src/worker-runtime/event-loop.js"
import {
  ControlServiceDefinition,
  type SendTaskResultRequest,
  Task,
} from "../src/worker-runtime/generated/control.js"
import { getEventHandlers, onStorageEvent } from "../src/worker-runtime/registry.js"

afterEach(() => getEventHandlers().clear())

it.each([
  { key: "images/雪.jpg", source: "objects", expected: "images", success: true },
  { key: "docs/a.txt", source: "objects", expected: "docs", success: true },
  { key: "Images/a.jpg", source: "objects", expected: undefined, success: true },
  { key: "other/a.txt", source: "objects", expected: undefined, success: true },
  { key: "images/a.jpg", source: "unregistered", expected: undefined, success: false },
])("filters storage keys through real gRPC: $source/$key", async testCase => {
  const results: SendTaskResultRequest[] = []
  const calls: string[] = []
  let loop: EventLoop
  const server = createServer()
  server.add(ControlServiceDefinition, {
    async registerHttpServer() {
      throw new Error("Unexpected HTTP registration")
    },
    async registerEventHandler() {
      return { handlerId: "storage", success: true }
    },
    async *waitForTasks() {
      yield Task.fromPartial({
        taskId: "storage-task",
        storageEvent: {
          bucket: testCase.source,
          key: testCase.key,
          eventType: "created",
          timestamp: new Date("2026-10-30T00:00:00Z"),
          size: 7,
        },
      })
    },
    async sendTaskResult(request) {
      results.push(request)
      loop.stop()
      return { acknowledged: true }
    },
  })
  const port = await server.listen("127.0.0.1:0")
  const endpoint = `127.0.0.1:${port}`
  const channel = createChannel(endpoint)
  for (const prefix of ["images", "docs"]) {
    onStorageEvent(
      "objects",
      async event => {
        expect(event.objectKey).toBe(testCase.key)
        calls.push(prefix)
      },
      { prefix: `${prefix}/` },
    )
  }
  loop = new EventLoop(channel, "application", endpoint, ControlServiceDefinition)
  try {
    await loop.registerHandlers()
    await loop.start()
    expect(calls).toEqual(testCase.expected === undefined ? [] : [testCase.expected])
    expect(results).toHaveLength(1)
    expect(results[0].taskId).toBe("storage-task")
    if (testCase.success) {
      expect(results[0].success).toBeDefined()
      expect(results[0].error).toBeUndefined()
    } else {
      expect(results[0].success).toBeUndefined()
      expect(results[0].error?.message).toContain("No handler registered")
    }
  } finally {
    loop.stop()
    channel.close()
    await server.shutdown()
  }
})
