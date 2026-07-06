/**
 * `@alienplatform/commands` — the public command package for TypeScript.
 *
 * Pure `fetch` over the command wire protocol: the command **sender**
 * ({@link CommandsClient}, with {@link CommandsClient.target}) and — added in a
 * follow-up task — the non-Worker pull **receiver** (`createCommandReceiver`).
 * No native code, no gRPC, no bindings.
 *
 * @example
 * ```typescript
 * import { CommandsClient } from "@alienplatform/commands"
 *
 * const commands = new CommandsClient({
 *   managerUrl: "https://manager.example.com",
 *   deploymentId: "deployment_123",
 *   token: "bearer_token",
 * })
 *
 * const result = await commands.invoke("generate-report", {
 *   startDate: "2024-01-01",
 *   endDate: "2024-01-31",
 * })
 *
 * // Or scoped to a specific target resource:
 * const scoped = commands.target("container-7")
 * await scoped.invoke("restart", {})
 * ```
 */

// Sender
export { CommandsClient, TargetedCommands } from "./client.js"
export type { CommandsClientConfig, InvokeOptions } from "./client.js"

// Receiver
export { createCommandReceiver } from "./receiver.js"
export type {
  CommandContext,
  CommandHandler,
  CommandReceiver,
  CommandReceiverOptions,
} from "./receiver.js"

// Error set
export {
  CommandCreationFailedError,
  CommandExpiredError,
  CommandReceiverConfigInvalidError,
  CommandTimeoutError,
  DeploymentCommandError,
  ManagerHttpError,
  ResponseDecodingFailedError,
  StorageOperationFailedError,
} from "./errors.js"

// Wire protocol types
export type {
  BodySpec,
  CommandResponse,
  CommandState,
  CommandStatusResponse,
  CommandTarget,
  CommandTargetType,
  CreateCommandRequest,
  CreateCommandResponse,
  Envelope,
  ErrorResponse,
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
} from "./protocol.js"
export { COMMANDS_INLINE_MAX_BYTES, COMMANDS_PROTOCOL_VERSION } from "./protocol.js"

// Shared error primitives, re-exported for consumer error handling.
export { AlienError, defineError } from "@alienplatform/core"
