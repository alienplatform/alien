/**
 * @aliendotdev/commands-client
 *
 * Lightweight client for Alien Commands protocol.
 *
 * @example
 * ```typescript
 * import { CommandsClient } from "@aliendotdev/commands-client"
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
 * ```
 *
 * @packageDocumentation
 */

export { CommandsClient } from "./client.js"
export type {
  CommandsClientConfig,
  BodySpec,
  CommandResponse,
  CommandState,
  CommandStatusResponse,
  CreateCommandResponse,
  InvokeOptions,
  PresignedRequest,
  StorageUpload,
} from "./types.js"
export {
  DeploymentCommandError,
  ManagerHttpError,
  CommandCreationFailedError,
  CommandExpiredError,
  CommandTimeoutError,
  ResponseDecodingFailedError,
  StorageOperationFailedError,
} from "./errors.js"
