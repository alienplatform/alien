/**
 * gRPC channel management for the Alien Worker runtime.
 */

import { AlienError } from "@alienplatform/core"
import type { ChannelOptions } from "@grpc/grpc-js"
import { type Channel, createChannel } from "nice-grpc"
import { GrpcConnectionError, MissingEnvVarError } from "./errors.js"

/** Default channel options for gRPC connections */
const DEFAULT_CHANNEL_OPTIONS: ChannelOptions = {
  // Explicit message size limits (128 MB) to avoid platform-specific defaults
  "grpc.max_send_message_length": 128 * 1024 * 1024,
  "grpc.max_receive_message_length": 128 * 1024 * 1024,
}

/** Environment variable containing the Worker protocol gRPC endpoint */
const GRPC_ENDPOINT_VAR = "ALIEN_WORKER_GRPC_ADDRESS"

/** Address name injected by released runtimes from before the Worker protocol rename. */
const LEGACY_GRPC_ENDPOINT_VAR = "ALIEN_BINDINGS_GRPC_ADDRESS"

export type WorkerProtocolGeneration = "current" | "legacy"

export interface GrpcEndpointConfig {
  address: string
  generation: WorkerProtocolGeneration
}

/** Cached channels by address */
const channelCache = new Map<string, Channel>()

/** Re-export the Channel type */
export type GrpcChannel = Channel

/**
 * Get the gRPC endpoint from environment variables.
 */
export function getGrpcEndpointConfig(): GrpcEndpointConfig {
  const currentEndpoint = process.env[GRPC_ENDPOINT_VAR]
  if (currentEndpoint) {
    return { address: currentEndpoint, generation: "current" }
  }

  const legacyEndpoint = process.env[LEGACY_GRPC_ENDPOINT_VAR]
  if (legacyEndpoint) {
    return { address: legacyEndpoint, generation: "legacy" }
  }

  throw new AlienError(
    MissingEnvVarError.create({
      variable: GRPC_ENDPOINT_VAR,
      description:
        "This variable is set by alien-worker-runtime when running inside the Alien environment.",
    }),
  )
}

export function getGrpcEndpoint(): string {
  return getGrpcEndpointConfig().address
}

/**
 * Create a gRPC channel to the specified address.
 */
export async function createGrpcChannel(address: string): Promise<Channel> {
  try {
    return createChannel(address, undefined, DEFAULT_CHANNEL_OPTIONS)
  } catch (error) {
    throw (await AlienError.from(error)).withContext(
      GrpcConnectionError.create({
        endpoint: address,
        reason: error instanceof Error ? error.message : String(error),
      }),
    )
  }
}

/**
 * Get or create a cached gRPC channel to the specified address.
 */
export async function getOrCreateChannel(address: string): Promise<Channel> {
  let channel = channelCache.get(address)
  if (!channel) {
    channel = await createGrpcChannel(address)
    channelCache.set(address, channel)
  }
  return channel
}
