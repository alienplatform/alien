import type { Deployment } from "@aliendotdev/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

const EXPECTED_EVENT_COUNT = 10

export async function checkSSE(agent: Deployment): Promise<void> {
  const response = await fetch(`${agent.url}/sse`)
  await assertResponseOk(response, "sse", "SSE endpoint request failed")
  assertCheck(response.body, "sse", "SSE response has no body")
  const contentType = response.headers.get("content-type")
  assertCheck(contentType?.includes("text/event-stream"), "sse", "Unexpected content-type", {
    contentType,
  })

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  const events: string[] = []
  let buffer = ""

  try {
    while (events.length < EXPECTED_EVENT_COUNT) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split("\n")
      buffer = lines.pop() || ""
      for (const line of lines) {
        if (line.startsWith("data: ")) {
          events.push(line.slice(6))
        }
      }
    }
  } finally {
    reader.releaseLock()
  }

  assertCheck(events.length === EXPECTED_EVENT_COUNT, "sse", "Unexpected event count", {
    expectedCount: EXPECTED_EVENT_COUNT,
    actualCount: events.length,
  })
  for (let i = 0; i < EXPECTED_EVENT_COUNT; i++) {
    assertCheck(events[i] === `sse_message_${i}`, "sse", "Event has unexpected data", {
      index: i,
      expectedValue: `sse_message_${i}`,
      actualValue: events[i],
    })
  }
}
