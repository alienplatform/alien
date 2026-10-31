import { AlienError } from "@alienplatform/core"
import { describe, expect, it } from "vitest"
import { storageEventFromMessage } from "../src/storage-events.js"

const event = {
  eventType: "created",
  bucketName: "objects",
  objectKey: 'in/雪/"quote"/*.json',
  timestamp: "2026-10-30T00:00:00Z",
  size: 7,
  etag: "content-hash",
}

describe("storageEventFromMessage", () => {
  it("decodes Worker deliveries and raw binding payloads without changing keys", async () => {
    expect(await storageEventFromMessage({ id: "message", payload: event })).toEqual(event)
    expect(await storageEventFromMessage({ payload: JSON.stringify(event) })).toEqual(event)
    const { size, etag, ...withoutOptionalFields } = event
    expect(await storageEventFromMessage({ payload: withoutOptionalFields })).toEqual(
      withoutOptionalFields,
    )
  })

  it.each([
    "not json",
    null,
    [],
    {},
    { ...event, eventType: "unsupported" },
    { ...event, timestamp: "yesterday" },
    { ...event, size: -1 },
    { ...event, size: 1.5 },
    { ...event, objectKey: undefined },
  ])("rejects invalid payloads with structured context: %j", async payload => {
    await expect(storageEventFromMessage({ id: "invalid", payload })).rejects.toMatchObject({
      code: "INVALID_STORAGE_EVENT_MESSAGE",
      context: { messageId: "invalid" },
      retryable: false,
    })
    await expect(storageEventFromMessage({ payload })).rejects.toBeInstanceOf(AlienError)
  })
})
