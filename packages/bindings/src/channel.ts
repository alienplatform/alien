/**
 * gRPC channel management for the Alien bindings SDK.
 */

import { AlienError } from "@aliendotdev/core"
import { type Channel, createChannel } from "nice-grpc"
import { GrpcConnectionError, MissingEnvVarError } from "./errors.js"

/** Environment variable containing the gRPC endpoint */
const GRPC_ENDPOINT_VAR = "ALIEN_BINDINGS_GRPC_ADDRESS"

/** Cached channels by address */
const channelCache = new Map<string, Channel>()

/** Re-export the Channel type */
export type GrpcChannel = Channel

/**
 * Get the gRPC endpoint from environment variables.
 */
export function getGrpcEndpoint(): string {
  const endpoint = process.env[GRPC_ENDPOINT_VAR]
  if (!endpoint) {
    throw new AlienError(
      MissingEnvVarError.create({
        variable: GRPC_ENDPOINT_VAR,
        description:
          "This variable is set by alien-runtime when running inside the Alien environment.",
      }),
    )
  }
  return endpoint
}

/**
 * Create a gRPC channel to the specified address.
 *
 * @param address - The gRPC server address
 * @returns The created channel
 */
export async function createGrpcChannel(address: string): Promise<Channel> {
  try {
    return createChannel(address)
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
 *
 * @param address - The gRPC server address
 * @returns The cached or newly created channel
 */
export async function getOrCreateChannel(address: string): Promise<Channel> {
  let channel = channelCache.get(address)
  if (!channel) {
    channel = await createGrpcChannel(address)
    channelCache.set(address, channel)
  }
  return channel
}

/**
 * Get a gRPC channel to the alien-runtime using the default env var.
 * The channel is cached for reuse across all bindings.
 */
export async function getChannel(): Promise<Channel> {
  const endpoint = getGrpcEndpoint()
  return await getOrCreateChannel(endpoint)
}

/**
 * Close all cached gRPC channels.
 * This should be called when shutting down the application.
 */
export function closeChannel(): void {
  for (const channel of channelCache.values()) {
    channel.close()
  }
  channelCache.clear()
}

/**
 * Reset the cached channels (useful for testing).
 */
export function resetChannel(): void {
  channelCache.clear()
}
