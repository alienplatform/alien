/**
 * ArtifactRegistry binding implementation.
 *
 * Provides container image repository management.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  ArtifactRegistryServiceDefinition,
  ComputeServiceType as ComputeServiceTypeProto,
  type Credentials,
  type CrossAccountAccess as CrossAccountAccessProto,
  type CrossAccountPermissions as CrossAccountPermissionsProto,
  type ArtifactRegistryServiceClient as GeneratedClient,
  ArtifactRegistryPermissions as PermissionsProto,
  type RepositoryResult,
} from "../generated/artifact_registry.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type {
  ArtifactRegistryCredentials,
  ArtifactRegistryPermissions,
  ComputeServiceType,
  CrossAccountAccess,
  CrossAccountPermissions,
  RepositoryInfo,
} from "../types.js"

// Permission map
const permissionMap: Record<ArtifactRegistryPermissions, PermissionsProto> = {
  pull: PermissionsProto.PULL,
  "push-pull": PermissionsProto.PUSH_PULL,
}

/**
 * ArtifactRegistry binding for container repository management.
 *
 * @example
 * ```typescript
 * import { artifactRegistry } from "@alienplatform/bindings"
 *
 * const registry = artifactRegistry("my-registry")
 *
 * // Create a repository
 * const repo = await registry.createRepository("my-app")
 * console.log("Repository URI:", repo.uri)
 *
 * // Generate push credentials
 * const creds = await registry.generateCredentials("my-app", "push-pull")
 * console.log("Username:", creds.username)
 * ```
 */
export class ArtifactRegistry {
  private readonly client: GeneratedClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(ArtifactRegistryServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Create a repository.
   *
   * @param repoName - Name of the repository
   * @returns Repository information
   */
  async createRepository(repoName: string): Promise<RepositoryInfo> {
    return await wrapGrpcCall(
      "ArtifactRegistryService",
      "CreateRepository",
      async () => {
        const response = await this.client.createRepository({
          bindingName: this.bindingName,
          repoName,
        })
        return this.fromProtoResult(response.result!)
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Get repository details.
   *
   * @param repoId - Repository ID
   * @returns Repository information
   */
  async getRepository(repoId: string): Promise<RepositoryInfo> {
    return await wrapGrpcCall(
      "ArtifactRegistryService",
      "GetRepository",
      async () => {
        const response = await this.client.getRepository({
          bindingName: this.bindingName,
          repoId,
        })
        return this.fromProtoResult(response.result!)
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Add cross-account access to a repository.
   *
   * @param repoId - Repository ID
   * @param access - Cross-account access configuration
   */
  async addCrossAccountAccess(repoId: string, access: CrossAccountAccess): Promise<void> {
    await wrapGrpcCall(
      "ArtifactRegistryService",
      "AddCrossAccountAccess",
      async () => {
        await this.client.addCrossAccountAccess({
          bindingName: this.bindingName,
          repoId,
          access: this.toProtoCrossAccountAccess(access),
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Remove cross-account access from a repository.
   *
   * @param repoId - Repository ID
   * @param access - Cross-account access configuration to remove
   */
  async removeCrossAccountAccess(repoId: string, access: CrossAccountAccess): Promise<void> {
    await wrapGrpcCall(
      "ArtifactRegistryService",
      "RemoveCrossAccountAccess",
      async () => {
        await this.client.removeCrossAccountAccess({
          bindingName: this.bindingName,
          repoId,
          access: this.toProtoCrossAccountAccess(access),
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Get current cross-account access permissions for a repository.
   *
   * @param repoId - Repository ID
   * @returns Current cross-account permissions
   */
  async getCrossAccountAccess(repoId: string): Promise<CrossAccountPermissions> {
    return await wrapGrpcCall(
      "ArtifactRegistryService",
      "GetCrossAccountAccess",
      async () => {
        const response = await this.client.getCrossAccountAccess({
          bindingName: this.bindingName,
          repoId,
        })
        return this.fromProtoCrossAccountPermissions(response.permissions!)
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Generate credentials for repository access.
   *
   * @param repoId - Repository ID
   * @param permissions - Permission level (pull or push-pull)
   * @param ttlSeconds - Optional TTL in seconds
   * @returns Repository credentials
   */
  async generateCredentials(
    repoId: string,
    permissions: ArtifactRegistryPermissions,
    ttlSeconds?: number,
  ): Promise<ArtifactRegistryCredentials> {
    return await wrapGrpcCall(
      "ArtifactRegistryService",
      "GenerateCredentials",
      async () => {
        const response = await this.client.generateCredentials({
          bindingName: this.bindingName,
          repoId,
          permissions: permissionMap[permissions],
          ttlSeconds,
        })
        return this.fromProtoCredentials(response.credentials!)
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Delete a repository.
   *
   * @param repoId - Repository ID
   */
  async deleteRepository(repoId: string): Promise<void> {
    await wrapGrpcCall(
      "ArtifactRegistryService",
      "DeleteRepository",
      async () => {
        await this.client.deleteRepository({
          bindingName: this.bindingName,
          repoId,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  // Private helpers

  private fromProtoResult(proto: RepositoryResult): RepositoryInfo {
    return {
      name: proto.name,
      uri: proto.uri ?? "",
      createdAt: proto.createdAt ? new Date(proto.createdAt) : undefined,
    }
  }

  private fromProtoCredentials(proto: Credentials): ArtifactRegistryCredentials {
    return {
      username: proto.username,
      password: proto.password,
      expiresAt: proto.expiresAt ? new Date(proto.expiresAt) : undefined,
    }
  }

  private toProtoCrossAccountAccess(access: CrossAccountAccess): CrossAccountAccessProto {
    if (access.type === "aws") {
      return {
        aws: {
          accountIds: access.aws.accountIds,
          allowedServiceTypes: access.aws.allowedServiceTypes.map(st =>
            this.computeServiceTypeToProto(st),
          ),
          roleArns: access.aws.roleArns,
        },
        gcp: undefined,
      }
    }
    return {
      aws: undefined,
      gcp: {
        projectNumbers: access.gcp.projectNumbers,
        allowedServiceTypes: access.gcp.allowedServiceTypes.map(st =>
          this.computeServiceTypeToProto(st),
        ),
        serviceAccountEmails: access.gcp.serviceAccountEmails,
      },
    }
  }

  private fromProtoCrossAccountAccess(proto: CrossAccountAccessProto): CrossAccountAccess {
    if (proto.aws) {
      return {
        type: "aws",
        aws: {
          accountIds: proto.aws.accountIds,
          allowedServiceTypes: proto.aws.allowedServiceTypes
            .map(st => this.protoToComputeServiceType(st))
            .filter((st): st is ComputeServiceType => st !== undefined),
          roleArns: proto.aws.roleArns,
        },
      }
    }
    if (proto.gcp) {
      return {
        type: "gcp",
        gcp: {
          projectNumbers: proto.gcp.projectNumbers,
          allowedServiceTypes: proto.gcp.allowedServiceTypes
            .map(st => this.protoToComputeServiceType(st))
            .filter((st): st is ComputeServiceType => st !== undefined),
          serviceAccountEmails: proto.gcp.serviceAccountEmails,
        },
      }
    }
    throw new Error("Invalid CrossAccountAccess: neither aws nor gcp set")
  }

  private fromProtoCrossAccountPermissions(
    proto: CrossAccountPermissionsProto,
  ): CrossAccountPermissions {
    if (!proto.access) {
      throw new Error("Invalid CrossAccountPermissions: access is missing")
    }
    return {
      access: this.fromProtoCrossAccountAccess(proto.access),
      lastUpdated: proto.lastUpdated,
    }
  }

  private computeServiceTypeToProto(st: ComputeServiceType): ComputeServiceTypeProto {
    switch (st) {
      case "function":
        return ComputeServiceTypeProto.FUNCTION
      default:
        return ComputeServiceTypeProto.UNRECOGNIZED
    }
  }

  private protoToComputeServiceType(st: ComputeServiceTypeProto): ComputeServiceType | undefined {
    switch (st) {
      case ComputeServiceTypeProto.FUNCTION:
        return "function"
      default:
        return undefined
    }
  }
}
