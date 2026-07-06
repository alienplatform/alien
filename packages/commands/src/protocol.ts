/**
 * Command wire protocol — TypeScript twin of the Rust serde shapes in
 * `crates/alien-core/src/commands_types.rs` and `crates/alien-core/src/presigned.rs`.
 *
 * These are the on-the-wire JSON shapes exchanged with the command server
 * (`/v1/commands…`). They are transcribed here so this package owns its
 * protocol surface with no cross-package type dependency; the field names and
 * tag values must stay byte-identical to the Rust side (both are
 * `#[serde(rename_all = "camelCase")]` unless noted).
 */

/** Command lifecycle state — SCREAMING_SNAKE_CASE on the wire. */
export type CommandState =
  | "PENDING_UPLOAD"
  | "PENDING"
  | "DISPATCHED"
  | "SUCCEEDED"
  | "FAILED"
  | "EXPIRED"

/** Payload container — internally tagged by `mode` (lowercase). */
export type BodySpec =
  | { mode: "inline"; inlineBase64: string }
  | {
      mode: "storage"
      size?: number | null
      storageGetRequest?: PresignedRequest
      storagePutUsed?: boolean
    }

/** Terminal command result — internally tagged by `status` (lowercase). */
export type CommandResponse =
  | { status: "success"; response: BodySpec }
  | { status: "error"; code: string; message: string; details?: string }

/** Target resource type — lowercase; `worker` is only valid for the worker runtime. */
export type CommandTargetType = "worker" | "container" | "daemon"

export type CommandTarget = {
  resourceId: string
  resourceType: CommandTargetType
}

export type CreateCommandRequest = {
  deploymentId: string
  command: string
  params: BodySpec
  deadline?: string /* RFC3339 */
  idempotencyKey?: string
  targetResourceId?: string
}

export type StorageUpload = {
  putRequest: PresignedRequest
  expiresAt: string
}

export type CreateCommandResponse = {
  commandId: string
  state: CommandState
  storageUpload?: StorageUpload
  inlineAllowedUpTo: number
  next: string /* "upload" | "poll" */
}

export type UploadCompleteRequest = { size: number }
export type UploadCompleteResponse = { commandId: string; state: CommandState }

export type CommandStatusResponse = {
  commandId: string
  state: CommandState
  attempt: number
  target: CommandTarget
  response?: CommandResponse
}

/**
 * `SubmitResponseRequest` is a `#[serde(flatten)]` of `CommandResponse` — the
 * wire body IS the `CommandResponse` itself.
 */
export type SubmitResponseRequest = CommandResponse

export type ResponseHandling = {
  maxInlineBytes: number
  submitResponseUrl: string
  storageUploadRequest: PresignedRequest
}

export type Envelope = {
  protocol: string /* "arc.v1" */
  deploymentId: string
  target: CommandTarget
  commandId: string
  attempt: number
  deadline?: string
  command: string
  params: BodySpec
  responseHandling: ResponseHandling
}

export type LeaseRequest = {
  deploymentId: string
  target: CommandTarget /* REQUIRED — deser fails without it */
  maxLeases?: number /* serde default 1 */
  leaseSeconds?: number /* default 60 */
}

export type LeaseInfo = {
  leaseId: string
  leaseExpiresAt: string
  commandId: string
  attempt: number
  envelope: Envelope
}

export type LeaseResponse = { leases: LeaseInfo[] }

export type ReleaseRequest = { leaseId: string }

/**
 * Presigned transfer descriptor (`presigned.rs`, camelCase; backend tagged by
 * `type`, camelCase).
 */
export type PresignedRequest = {
  backend:
    | { type: "http"; url: string; method: string; headers: Record<string, string> }
    | { type: "local"; filePath: string; operation: "put" | "get" | "delete" }
  expiration: string
  operation: "put" | "get" | "delete"
  path: string
}

/** Error envelope returned by the axum handlers on non-2xx responses. */
export type ErrorResponse = { code: string; message: string; details?: string }

/** Protocol version carried by command envelopes. */
export const COMMANDS_PROTOCOL_VERSION = "arc.v1"

/** Server inline payload ceiling (bytes). */
export const COMMANDS_INLINE_MAX_BYTES = 150_000
