/**
 * ServiceAccount binding implementation.
 *
 * Provides identity and impersonation capabilities.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type ServiceAccountServiceClient as GeneratedClient,
  type ServiceAccountInfo as ServiceAccountInfoProto,
  ServiceAccountServiceDefinition,
} from "../generated/service_account.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type { ImpersonationRequest, ServiceAccountInfo } from "../types.js"

/**
 * ServiceAccount binding for identity and impersonation.
 *
 * @example
 * ```typescript
 * import { serviceAccount } from "@aliendotdev/bindings"
 *
 * const sa = serviceAccount("deployment-account")
 *
 * // Get account info
 * const info = await sa.getInfo()
 * if (info.platform === "aws") {
 *   console.log("Role ARN:", info.roleArn)
 * }
 *
 * // Impersonate the service account
 * const config = await sa.impersonate({ durationSeconds: 3600 })
 * ```
 */
export class ServiceAccount {
  private readonly client: GeneratedClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(ServiceAccountServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Get information about the service account.
   *
   * @returns Platform-specific service account information
   */
  async getInfo(): Promise<ServiceAccountInfo> {
    return await wrapGrpcCall(
      "ServiceAccountService",
      "GetInfo",
      async () => {
        const response = await this.client.getInfo({
          bindingName: this.bindingName,
        })
        return this.fromProtoInfo(response.info!)
      },
      { bindingName: this.bindingName, bindingType: "ServiceAccount" },
    )
  }

  /**
   * Impersonate the service account.
   *
   * @param request - Impersonation options
   * @returns Client configuration for the impersonated identity
   */
  async impersonate(request?: ImpersonationRequest): Promise<unknown> {
    return await wrapGrpcCall(
      "ServiceAccountService",
      "Impersonate",
      async () => {
        const response = await this.client.impersonate({
          bindingName: this.bindingName,
          sessionName: request?.sessionName,
          durationSeconds: request?.durationSeconds,
          scopes: request?.scopes ?? [],
        })
        return JSON.parse(response.clientConfigJson)
      },
      { bindingName: this.bindingName, bindingType: "ServiceAccount" },
    )
  }

  // Private helpers

  private fromProtoInfo(proto: ServiceAccountInfoProto): ServiceAccountInfo {
    if (proto.aws) {
      return {
        platform: "aws",
        roleName: proto.aws.roleName,
        roleArn: proto.aws.roleArn,
      }
    }
    if (proto.gcp) {
      return {
        platform: "gcp",
        email: proto.gcp.email,
        uniqueId: proto.gcp.uniqueId,
      }
    }
    if (proto.azure) {
      return {
        platform: "azure",
        clientId: proto.azure.clientId,
        resourceId: proto.azure.resourceId,
        principalId: proto.azure.principalId,
      }
    }
    throw new Error("Unknown service account platform")
  }
}
