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
  DeployOptions,
  DeploymentInfo,
  Platform,
  EnvironmentVariable,
  PlatformCredentials,
  AWSCredentials,
  GCPCredentials,
  AzureCredentials,
  KubernetesCredentials,
  LocalCredentials,
  TestCredentials,
} from "./types.js"
