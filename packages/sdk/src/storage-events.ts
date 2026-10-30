import { AlienError, defineError, type StorageEvent, StorageEventSchema } from "@alienplatform/core"
import { z } from "zod/v4"

/** A queue payload cannot be decoded as the portable storage event. */
const InvalidStorageEventMessage = defineError({
  code: "INVALID_STORAGE_EVENT_MESSAGE",
  context: z.object({ messageId: z.string().optional() }),
  message: ({ messageId }) =>
    messageId === undefined
      ? "Queue payload is not a valid StorageEvent"
      : `Queue message '${messageId}' is not a valid StorageEvent`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/**
 * Decode a storage notification from either a Worker queue delivery or `queue.receive()`.
 * Validates the canonical event schema, including the timestamp and optional object size.
 * This does not acknowledge the message; acknowledge only after processing succeeds.
 */
export async function storageEventFromMessage(message: {
  payload: unknown
  id?: string
}): Promise<StorageEvent> {
  try {
    const payload: unknown =
      typeof message.payload === "string" ? JSON.parse(message.payload) : message.payload
    return StorageEventSchema.parse(payload)
  } catch (error) {
    throw (await AlienError.from(error)).withContext(
      InvalidStorageEventMessage.create({ messageId: message.id }),
    )
  }
}
