import type { Deployment } from "@aliendotdev/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

interface EventListResponse {
  storageEvents: unknown[]
  cronEvents: unknown[]
  queueMessages: unknown[]
}

interface EventResponse {
  found: boolean
  event?: unknown
}

/**
 * Verify the event handler registration endpoint works.
 * Doesn't assert events exist — just that the endpoint returns the expected structure.
 */
export async function checkStorageEventHandler(deployment: Deployment): Promise<void> {
  const response = await fetch(`${deployment.url}/events/list`)

  await assertResponseOk(response, "events-list", "Events list request failed")

  const data = (await response.json()) as EventListResponse

  assertCheck(
    Array.isArray(data.storageEvents),
    "events-list",
    "Invalid storage events array in response",
  )
  assertCheck(
    Array.isArray(data.queueMessages),
    "events-list",
    "Invalid queue messages array in response",
  )
}

export async function checkStorageEvent(deployment: Deployment, key: string): Promise<boolean> {
  const response = await fetch(`${deployment.url}/events/storage/${encodeURIComponent(key)}`)

  await assertResponseOk(response, "events-storage", "Storage event lookup request failed")

  const data = (await response.json()) as EventResponse
  return data.found
}

export async function checkQueueMessage(
  deployment: Deployment,
  messageId: string,
): Promise<boolean> {
  const response = await fetch(`${deployment.url}/events/queue/${encodeURIComponent(messageId)}`)

  await assertResponseOk(response, "events-queue", "Queue message lookup request failed")

  const data = (await response.json()) as EventResponse
  return data.found
}
