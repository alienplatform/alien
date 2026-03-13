/**
 * KV binding implementation.
 *
 * Provides key-value storage operations with TTL support.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type KvServiceClient as GeneratedKvServiceClient,
  KvServiceDefinition,
} from "../generated/kv.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type { KvPutOptions, KvScanResult } from "../types.js"

/**
 * KV binding for key-value storage operations.
 *
 * @example
 * ```typescript
 * import { kv } from "@aliendotdev/bindings"
 *
 * const cache = kv("my-cache")
 *
 * // Set a value
 * await cache.set("user:123", { name: "John", email: "john@example.com" })
 *
 * // Get a value
 * const user = await cache.getJson<User>("user:123")
 *
 * // Set with TTL (expires in 1 hour)
 * await cache.set("session:abc", sessionData, { ttlMs: 60 * 60 * 1000 })
 *
 * // Delete
 * await cache.delete("user:123")
 *
 * // Scan by prefix
 * for await (const { key, value } of cache.scan("user:")) {
 *   console.log(key, value)
 * }
 * ```
 */
export class Kv {
  private readonly client: GeneratedKvServiceClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(KvServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Get a value by key.
   *
   * @param key - Key to retrieve
   * @returns Value as Uint8Array, or undefined if not found
   */
  async get(key: string): Promise<Uint8Array | undefined> {
    return await wrapGrpcCall(
      "KvService",
      "Get",
      async () => {
        const response = await this.client.get({
          bindingName: this.bindingName,
          key,
        })
        return response.value
      },
      { bindingName: this.bindingName, key },
    )
  }

  /**
   * Get a value as a UTF-8 string.
   *
   * @param key - Key to retrieve
   * @returns Value as string, or undefined if not found
   */
  async getText(key: string): Promise<string | undefined> {
    const value = await this.get(key)
    return value ? new TextDecoder().decode(value) : undefined
  }

  /**
   * Get a value and parse as JSON.
   *
   * @param key - Key to retrieve
   * @returns Parsed JSON value, or undefined if not found
   */
  async getJson<T = unknown>(key: string): Promise<T | undefined> {
    const text = await this.getText(key)
    return text ? (JSON.parse(text) as T) : undefined
  }

  /**
   * Set a value.
   *
   * @param key - Key to store
   * @param value - Value to store (string, Uint8Array, or object for JSON)
   * @param options - Optional put options (TTL, ifNotExists)
   * @returns True if stored, false if ifNotExists was true and key already exists
   */
  async set(
    key: string,
    value: string | Uint8Array | object,
    options?: KvPutOptions,
  ): Promise<boolean> {
    let bytes: Uint8Array

    if (typeof value === "string") {
      bytes = new TextEncoder().encode(value)
    } else if (value instanceof Uint8Array) {
      bytes = value
    } else {
      bytes = new TextEncoder().encode(JSON.stringify(value))
    }

    return await wrapGrpcCall(
      "KvService",
      "Put",
      async () => {
        const response = await this.client.put({
          bindingName: this.bindingName,
          key,
          value: bytes,
          options: options
            ? {
                ttlSeconds: options.ttlMs ? Math.floor(options.ttlMs / 1000) : undefined,
                ifNotExists: options.ifNotExists ?? false,
              }
            : undefined,
        })
        return response.success
      },
      { bindingName: this.bindingName, key },
    )
  }

  /**
   * Delete a key.
   *
   * @param key - Key to delete
   */
  async delete(key: string): Promise<void> {
    await wrapGrpcCall(
      "KvService",
      "Delete",
      async () => {
        await this.client.delete({
          bindingName: this.bindingName,
          key,
        })
      },
      { bindingName: this.bindingName, key },
    )
  }

  /**
   * Check if a key exists.
   *
   * @param key - Key to check
   * @returns True if the key exists
   */
  async exists(key: string): Promise<boolean> {
    return await wrapGrpcCall(
      "KvService",
      "Exists",
      async () => {
        const response = await this.client.exists({
          bindingName: this.bindingName,
          key,
        })
        return response.exists
      },
      { bindingName: this.bindingName, key },
    )
  }

  /**
   * Scan keys with a prefix.
   *
   * @param prefix - Prefix to scan for
   * @param options - Optional scan options (limit)
   * @returns Async iterable of key-value pairs
   *
   * @example
   * ```typescript
   * // Scan all users
   * for await (const { key, value } of cache.scan("user:")) {
   *   console.log(key, new TextDecoder().decode(value))
   * }
   * ```
   */
  async *scan(
    prefix: string,
    options?: { limit?: number },
  ): AsyncIterable<{ key: string; value: Uint8Array }> {
    let cursor: string | undefined

    do {
      const result = await this.scanPage(prefix, options?.limit, cursor)

      for (const item of result.items) {
        yield item
      }

      cursor = result.nextCursor
    } while (cursor)
  }

  /**
   * Scan a single page of results.
   *
   * @param prefix - Prefix to scan for
   * @param limit - Maximum items to return
   * @param cursor - Pagination cursor
   * @returns Scan result with items and next cursor
   */
  async scanPage(prefix: string, limit?: number, cursor?: string): Promise<KvScanResult> {
    return await wrapGrpcCall(
      "KvService",
      "ScanPrefix",
      async () => {
        const response = await this.client.scanPrefix({
          bindingName: this.bindingName,
          prefix,
          limit,
          cursor,
        })
        return {
          items: response.items.map(item => ({
            key: item.key,
            value: item.value,
          })),
          nextCursor: response.nextCursor,
        }
      },
      { bindingName: this.bindingName },
    )
  }
}
