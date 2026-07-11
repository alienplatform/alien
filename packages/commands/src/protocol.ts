/**
 * Command wire protocol — the on-the-wire JSON shapes exchanged with the command
 * server (`/v1/commands…`).
 *
 * These shapes are the Kubb-generated types in `@alienplatform/core` (themselves
 * generated from the same OpenAPI schema the Rust serde shapes in
 * `crates/alien-core/src/commands_types.rs` and `presigned.rs` are pinned to), so
 * they are re-exported here rather than hand-transcribed — the commands package
 * keeps one named protocol surface without duplicating a second copy that could
 * drift from the generated one. Only the constants and the handful of shapes core
 * does not surface publicly live locally below.
 */

export type {
  BodySpec,
  CommandResponse,
  CommandState,
  CommandStatusResponse,
  CreateCommandRequest,
  CreateCommandResponse,
  Envelope,
  LeaseInfo,
  LeaseRequest,
  LeaseResponse,
  PresignedRequest,
  ReleaseRequest,
  ResponseHandling,
  StorageUpload,
  SubmitResponseRequest,
  UploadCompleteRequest,
  UploadCompleteResponse,
} from "@alienplatform/core"

/**
 * Target resource type — lowercase; `worker` is only valid for the worker
 * runtime. Not surfaced from `@alienplatform/core`'s public API, so it stays
 * local; kept structurally identical to the generated `CommandTargetType`.
 */
export type CommandTargetType = "worker" | "container" | "daemon"

/** Identifies the specific resource a command is addressed to. */
export type CommandTarget = {
  resourceId: string
  resourceType: CommandTargetType
}

/** Error envelope returned by the axum handlers on non-2xx responses. */
export type ErrorResponse = { code: string; message: string; details?: string }
