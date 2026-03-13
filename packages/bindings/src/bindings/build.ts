/**
 * Build binding implementation.
 *
 * Provides build execution capabilities.
 */

import type { BuildStatus } from "@aliendotdev/core"
import { type Channel, createClient } from "nice-grpc"
import {
  type BuildConfig as BuildConfigProto,
  type BuildExecution as BuildExecutionProto,
  BuildServiceDefinition,
  BuildStatus as BuildStatusProto,
  ComputeType,
  type BuildServiceClient as GeneratedBuildServiceClient,
} from "../generated/build.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type { BuildExecution, BuildStartConfig } from "../types.js"

// Map proto status to core BuildStatus (matches Rust: Unspecified → Failed)
const statusMap: Record<number, BuildStatus> = {
  [BuildStatusProto.BUILD_STATUS_UNSPECIFIED]: "FAILED",
  [BuildStatusProto.BUILD_STATUS_QUEUED]: "QUEUED",
  [BuildStatusProto.BUILD_STATUS_RUNNING]: "RUNNING",
  [BuildStatusProto.BUILD_STATUS_SUCCEEDED]: "SUCCEEDED",
  [BuildStatusProto.BUILD_STATUS_FAILED]: "FAILED",
  [BuildStatusProto.BUILD_STATUS_CANCELLED]: "CANCELLED",
  [BuildStatusProto.BUILD_STATUS_TIMED_OUT]: "TIMED_OUT",
}

// Map compute type to proto
const computeTypeMap: Record<string, ComputeType> = {
  small: ComputeType.COMPUTE_TYPE_SMALL,
  medium: ComputeType.COMPUTE_TYPE_MEDIUM,
  large: ComputeType.COMPUTE_TYPE_LARGE,
  "x-large": ComputeType.COMPUTE_TYPE_XLARGE,
}

/**
 * Build binding for executing build operations.
 *
 * @example
 * ```typescript
 * import { build } from "@aliendotdev/bindings"
 *
 * const builder = build("my-builder")
 *
 * // Start a build
 * const execution = await builder.start({
 *   script: "npm run build",
 *   computeType: "medium",
 *   timeoutSeconds: 600,
 * })
 *
 * // Wait for completion
 * const result = await builder.waitForCompletion(execution.id)
 * console.log("Build status:", result.status)
 * ```
 */
export class Build {
  private readonly client: GeneratedBuildServiceClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(BuildServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Start a new build.
   *
   * @param config - Build configuration
   * @returns Build execution information
   */
  async start(config: BuildStartConfig): Promise<BuildExecution> {
    const protoConfig = this.toProtoConfig(config)

    return await wrapGrpcCall(
      "BuildService",
      "StartBuild",
      async () => {
        const response = await this.client.startBuild({
          bindingName: this.bindingName,
          config: protoConfig,
        })
        return this.fromProtoExecution(response.execution!)
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Get the status of a build.
   *
   * @param buildId - Build execution ID
   * @returns Build execution information
   */
  async getStatus(buildId: string): Promise<BuildExecution> {
    return await wrapGrpcCall(
      "BuildService",
      "GetBuildStatus",
      async () => {
        const response = await this.client.getBuildStatus({
          bindingName: this.bindingName,
          buildId,
        })
        return this.fromProtoExecution(response.execution!)
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Stop a running build.
   *
   * @param buildId - Build execution ID
   */
  async stop(buildId: string): Promise<void> {
    await wrapGrpcCall(
      "BuildService",
      "StopBuild",
      async () => {
        await this.client.stopBuild({
          bindingName: this.bindingName,
          buildId,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Wait for a build to complete.
   *
   * @param buildId - Build execution ID
   * @param options - Polling options
   * @returns Final build execution information
   */
  async waitForCompletion(
    buildId: string,
    options?: { pollIntervalMs?: number; timeoutMs?: number },
  ): Promise<BuildExecution> {
    const pollInterval = options?.pollIntervalMs ?? 5000
    const timeout = options?.timeoutMs ?? 30 * 60 * 1000 // 30 minutes default
    const startTime = Date.now()

    while (Date.now() - startTime < timeout) {
      const execution = await this.getStatus(buildId)

      if (
        execution.status === "SUCCEEDED" ||
        execution.status === "FAILED" ||
        execution.status === "CANCELLED" ||
        execution.status === "TIMED_OUT"
      ) {
        return execution
      }

      await new Promise(resolve => setTimeout(resolve, pollInterval))
    }

    throw new Error(`Build ${buildId} did not complete within timeout`)
  }

  // Private helpers

  private toProtoConfig(config: BuildStartConfig): BuildConfigProto {
    return {
      script: config.script,
      environment: config.environment ?? {},
      computeType: config.computeType
        ? (computeTypeMap[config.computeType] ?? ComputeType.COMPUTE_TYPE_UNSPECIFIED)
        : ComputeType.COMPUTE_TYPE_UNSPECIFIED,
      timeoutSeconds: config.timeoutSeconds,
      monitoring: config.monitoring
        ? {
            endpoint: config.monitoring.endpoint,
            headers: config.monitoring.headers ?? {},
            logsUri: config.monitoring.logsUri ?? "/v1/logs",
            tlsEnabled: config.monitoring.tlsEnabled ?? true,
            tlsVerify: config.monitoring.tlsVerify ?? true,
          }
        : undefined,
    }
  }

  private fromProtoExecution(proto: BuildExecutionProto): BuildExecution {
    return {
      id: proto.id,
      status: statusMap[proto.status] ?? "QUEUED",
      startTime: proto.startTime ? new Date(proto.startTime) : undefined,
      endTime: proto.endTime ? new Date(proto.endTime) : undefined,
    }
  }
}
