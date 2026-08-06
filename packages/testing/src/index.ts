/**
 * @alienplatform/testing — Testing framework for Alien applications
 */

export { deploy } from "./deploy.js"
export { Deployment } from "./deployment.js"
export {
  TestingOperationFailedError,
  TestingUnsupportedPlatformError,
} from "./errors.js"
export type {
  DeploymentInfo,
  DeployOptions,
  EnvironmentVariable,
  Platform,
  UpgradeOptions,
} from "./types.js"
