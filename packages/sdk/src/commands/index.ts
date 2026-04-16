/**
 * Commands client — invoke commands on remote Alien deployments.
 *
 * @example
 * ```typescript
 * import { CommandsClient } from "@alienplatform/sdk/commands"
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
