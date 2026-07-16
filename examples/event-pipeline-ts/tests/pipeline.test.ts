import { type Deployment, deploy } from "@alienplatform/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

type QueueEvents = {
  count: number
  events: Array<{ value: { payload: string } }>
}

type SendResult = { sent: boolean }
type EventStats = { queue: number }

describe("event-pipeline-ts", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: ".", platform: "local" })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("should process queue messages end-to-end", async () => {
    // Send a message to the queue via command
    const sendResult = await deployment.invokeCommand<SendResult>(
      "send-test-message",
      {
        message: "hello from queue",
      },
    )
    expect(sendResult.sent).toBe(true)

    // Wait for the LocalTriggerService to poll the queue and deliver
    // the message to the onQueueMessage handler
    let events: QueueEvents = { count: 0, events: [] }
    for (let i = 0; i < 15; i++) {
      await new Promise(resolve => setTimeout(resolve, 2000))
      events = await deployment.invokeCommand<QueueEvents>("get-events", {
        type: "queue",
      })
      if (events.count > 0) break
    }

    expect(events.count).toBeGreaterThan(0)
    expect(events.events[0].value.payload).toBe("hello from queue")
  })

  it("should track event stats after processing", async () => {
    const stats = await deployment.invokeCommand<EventStats>("get-stats", {})
    expect(stats.queue).toBeGreaterThanOrEqual(1)
  })

  it("should return empty results for missing event type", async () => {
    const events = await deployment.invokeCommand<QueueEvents>("get-events", {
      type: "nonexistent",
    })
    expect(events.count).toBe(0)
  })
})
