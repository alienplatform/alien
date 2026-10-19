import { createChannel, createServer } from "nice-grpc"
import { afterEach, expect, it } from "vitest"
import { EventLoop } from "../src/worker-runtime/event-loop.js"
import {
  ControlServiceDefinition,
  type SendTaskResultRequest,
  Task,
} from "../src/worker-runtime/generated/control.js"
import { getEventHandlers, onQueueBatch } from "../src/worker-runtime/registry.js"

afterEach(() => getEventHandlers().clear())

it.each(["jobs", "*"])("dispatches batches through real gRPC to %s", async source => {
  const results: SendTaskResultRequest[] = []
  const registrations: string[] = []
  let loop: EventLoop
  const server = createServer()
  server.add(ControlServiceDefinition, {
    async registerHttpServer() {
      throw new Error("Unexpected HTTP registration")
    },
    async registerEventHandler(request) {
      registrations.push(`${request.handlerType}:${request.resourceName}`)
      return { handlerId: "handler", success: true }
    },
    async *waitForTasks() {
      yield Task.fromPartial({
        taskId: "batch-task",
        queueBatch: {
          source: "jobs",
          deliveries: ["attempt-a", "attempt-b"].map(deliveryId => ({
            deliveryId,
            message: { id: "same-provider-id", source: "jobs", payload: Buffer.from('{"job":1}') },
          })),
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
  onQueueBatch(source, async batch => {
    expect(batch.source).toBe("jobs")
    expect(batch.deliveries).toHaveLength(2)
    expect(batch.deliveries[0].message.id).toBe(batch.deliveries[1].message.id)
    expect(batch.deliveries[0].deliveryId).not.toBe(batch.deliveries[1].deliveryId)
    expect(batch.deliveries[0].message.payload).toEqual({ job: 1 })
    return batch.deliveries.map((delivery, index) => ({
      deliveryId: delivery.deliveryId,
      disposition: index === 0 ? "success" : "retry",
    }))
  })
  loop = new EventLoop(channel, "application", endpoint, ControlServiceDefinition)
  try {
    await loop.registerHandlers()
    await loop.start()
    expect(registrations).toEqual([`queueBatch:${source}`])
    expect(results).toHaveLength(1)
    expect(results[0].taskId).toBe("batch-task")
    expect(results[0].queueBatch?.outcomes).toEqual([
      { deliveryId: "attempt-a", disposition: 1 },
      { deliveryId: "attempt-b", disposition: 2 },
    ])
    expect(results[0].success).toBeUndefined()
    expect(results[0].error).toBeUndefined()
  } finally {
    loop.stop()
    channel.close()
    await server.shutdown()
  }
})
