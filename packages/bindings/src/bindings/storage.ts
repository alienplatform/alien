/**
 * Storage binding implementation.
 *
 * Provides object storage operations with streaming support.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type StorageServiceClient as GeneratedClient,
  type StorageObjectMeta as ProtoObjectMeta,
  StorageHttpMethod,
  StoragePutModeEnum,
  type StoragePutMultipartChunkRequest,
  StorageServiceDefinition,
} from "../generated/storage.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type {
  SignedUrlOptions,
  SignedUrlResult,
  StorageGetOptions,
  StorageGetResult,
  StorageListResult,
  StorageObjectMeta,
  StoragePutOptions,
} from "../types.js"

/**
 * Storage binding for object storage operations.
 *
 * @example
 * ```typescript
 * import { storage } from "@alienplatform/bindings"
 *
 * const bucket = storage("my-bucket")
 *
 * // Upload a file
 * await bucket.put("images/photo.jpg", imageData, { contentType: "image/jpeg" })
 *
 * // Download a file
 * const result = await bucket.get("images/photo.jpg")
 * console.log("Size:", result.meta.size)
 *
 * // List files
 * for await (const file of bucket.list("images/")) {
 *   console.log(file.location)
 * }
 *
 * // Generate a signed URL
 * const { url } = await bucket.signedUrl("images/photo.jpg", { operation: "get" })
 * ```
 */
export class Storage {
  private readonly client: GeneratedClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(StorageServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Get an object from storage.
   *
   * @param path - Path to the object
   * @param options - Get options (range, conditionals)
   * @returns Object data and metadata
   */
  async get(path: string, options?: StorageGetOptions): Promise<StorageGetResult> {
    return await wrapGrpcCall(
      "StorageService",
      "Get",
      async () => {
        const stream = this.client.get({
          bindingName: this.bindingName,
          path,
          options: options
            ? {
                ifMatch: options.ifMatch,
                ifNoneMatch: options.ifNoneMatch,
                ifModifiedSince: options.ifModifiedSince,
                ifUnmodifiedSince: options.ifUnmodifiedSince,
                range:
                  options.rangeStart !== undefined || options.rangeEnd !== undefined
                    ? {
                        bounded: {
                          start: options.rangeStart ?? 0,
                          end: options.rangeEnd ?? 0,
                        },
                      }
                    : undefined,
                head: false,
              }
            : undefined,
        })

        let meta: StorageObjectMeta | undefined
        const chunks: Uint8Array[] = []

        for await (const part of stream) {
          if (part.metadata) {
            meta = this.fromProtoMeta(part.metadata)
          }
          if (part.chunkData && part.chunkData.length > 0) {
            chunks.push(part.chunkData)
          }
        }

        if (!meta) {
          throw new Error("No metadata received from storage")
        }

        // Combine chunks
        const totalLength = chunks.reduce((sum, chunk) => sum + chunk.length, 0)
        const data = new Uint8Array(totalLength)
        let offset = 0
        for (const chunk of chunks) {
          data.set(chunk, offset)
          offset += chunk.length
        }

        return { meta, data }
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Get an object as a UTF-8 string.
   *
   * @param path - Path to the object
   * @param options - Get options
   * @returns Object content as string
   */
  async getText(path: string, options?: StorageGetOptions): Promise<string> {
    const result = await this.get(path, options)
    return new TextDecoder().decode(result.data)
  }

  /**
   * Get an object and parse as JSON.
   *
   * @param path - Path to the object
   * @param options - Get options
   * @returns Parsed JSON content
   */
  async getJson<T = unknown>(path: string, options?: StorageGetOptions): Promise<T> {
    const text = await this.getText(path, options)
    return JSON.parse(text) as T
  }

  /**
   * Put an object to storage.
   *
   * @param path - Path to store the object
   * @param data - Object data
   * @param options - Put options (content type, metadata)
   */
  async put(
    path: string,
    data: Uint8Array | string | object,
    options?: StoragePutOptions,
  ): Promise<void> {
    let bytes: Uint8Array
    let contentType = options?.contentType

    if (typeof data === "string") {
      bytes = new TextEncoder().encode(data)
      contentType ??= "text/plain; charset=utf-8"
    } else if (data instanceof Uint8Array) {
      bytes = data
      contentType ??= "application/octet-stream"
    } else {
      bytes = new TextEncoder().encode(JSON.stringify(data))
      contentType ??= "application/json"
    }

    // Build attributes for content-type and metadata
    const attributePairs: Array<{ key: string; value: string }> = []
    if (contentType) {
      attributePairs.push({ key: "content-type", value: contentType })
    }
    if (options?.metadata) {
      for (const [key, value] of Object.entries(options.metadata)) {
        attributePairs.push({ key: `metadata:${key}`, value })
      }
    }

    await wrapGrpcCall(
      "StorageService",
      "Put",
      async () => {
        await this.client.put({
          bindingName: this.bindingName,
          path,
          data: bytes,
          options: {
            mode: options?.ifNotExists
              ? StoragePutModeEnum.PUT_MODE_CREATE
              : StoragePutModeEnum.PUT_MODE_OVERWRITE,
            attributes: attributePairs.length > 0 ? { pairs: attributePairs } : undefined,
          },
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Put an object using streaming (for large files).
   *
   * @param path - Path to store the object
   * @param chunks - Async iterable of data chunks
   * @param options - Put options
   */
  async putMultipart(
    path: string,
    chunks: AsyncIterable<Uint8Array>,
    options?: StoragePutOptions,
  ): Promise<void> {
    const bindingName = this.bindingName

    // Build attributes for content-type and metadata
    const attributePairs: Array<{ key: string; value: string }> = []
    if (options?.contentType) {
      attributePairs.push({ key: "content-type", value: options.contentType })
    }
    if (options?.metadata) {
      for (const [key, value] of Object.entries(options.metadata)) {
        attributePairs.push({ key: `metadata:${key}`, value })
      }
    }

    await wrapGrpcCall(
      "StorageService",
      "PutMultipart",
      async () => {
        async function* generateChunks(): AsyncIterable<StoragePutMultipartChunkRequest> {
          // First chunk: metadata
          yield {
            metadata: {
              bindingName,
              path,
              options:
                attributePairs.length > 0 ? { attributes: { pairs: attributePairs } } : undefined,
            },
            chunkData: undefined,
          }

          // Subsequent chunks: data
          for await (const chunk of chunks) {
            yield {
              metadata: undefined,
              chunkData: chunk,
            }
          }
        }

        await this.client.putMultipart(generateChunks())
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Delete an object from storage.
   *
   * @param path - Path to the object
   */
  async delete(path: string): Promise<void> {
    await wrapGrpcCall(
      "StorageService",
      "Delete",
      async () => {
        await this.client.delete({
          bindingName: this.bindingName,
          path,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * List objects in storage.
   *
   * @param prefix - Optional prefix filter
   * @param options - List options
   * @returns Async iterable of object metadata
   */
  async *list(prefix?: string, options?: { offset?: string }): AsyncIterable<StorageObjectMeta> {
    const stream = this.client.list({
      bindingName: this.bindingName,
      prefix,
      offset: options?.offset,
    })

    for await (const meta of stream) {
      yield this.fromProtoMeta(meta)
    }
  }

  /**
   * List objects with delimiter (for directory-like listing).
   *
   * @param prefix - Optional prefix filter
   * @returns List result with objects and common prefixes
   */
  async listWithDelimiter(prefix?: string): Promise<StorageListResult> {
    return await wrapGrpcCall(
      "StorageService",
      "ListWithDelimiter",
      async () => {
        const response = await this.client.listWithDelimiter({
          bindingName: this.bindingName,
          prefix,
        })
        return {
          commonPrefixes: response.commonPrefixes,
          objects: response.objects.map(obj => this.fromProtoMeta(obj)),
        }
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Get object metadata without downloading content.
   *
   * @param path - Path to the object
   * @returns Object metadata
   */
  async head(path: string): Promise<StorageObjectMeta> {
    return await wrapGrpcCall(
      "StorageService",
      "Head",
      async () => {
        const response = await this.client.head({
          bindingName: this.bindingName,
          path,
        })
        return this.fromProtoMeta(response)
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Check if an object exists.
   *
   * @param path - Path to the object
   * @returns True if the object exists
   */
  async exists(path: string): Promise<boolean> {
    try {
      await this.head(path)
      return true
    } catch (error) {
      if (error instanceof Error && "code" in error && (error as any).code === "NOT_FOUND") {
        return false
      }
      throw error
    }
  }

  /**
   * Get the base directory path for this storage binding.
   *
   * @returns Base directory path
   */
  async getBaseDir(): Promise<string> {
    return await wrapGrpcCall(
      "StorageService",
      "GetBaseDir",
      async () => {
        const response = await this.client.getBaseDir({
          bindingName: this.bindingName,
        })
        return response.path
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Get the underlying URL for this storage binding.
   *
   * @returns Storage URL
   */
  async getUrl(): Promise<string> {
    return await wrapGrpcCall(
      "StorageService",
      "GetUrl",
      async () => {
        const response = await this.client.getUrl({
          bindingName: this.bindingName,
        })
        return response.url
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Copy an object within the same storage binding.
   *
   * @param from - Source path
   * @param to - Destination path
   */
  async copy(from: string, to: string): Promise<void> {
    await wrapGrpcCall(
      "StorageService",
      "Copy",
      async () => {
        await this.client.copy({
          bindingName: this.bindingName,
          fromPath: from,
          toPath: to,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Rename (move) an object within the same storage binding.
   *
   * @param from - Source path
   * @param to - Destination path
   */
  async rename(from: string, to: string): Promise<void> {
    await wrapGrpcCall(
      "StorageService",
      "Rename",
      async () => {
        await this.client.rename({
          bindingName: this.bindingName,
          fromPath: from,
          toPath: to,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Copy an object only if the destination doesn't exist.
   *
   * @param from - Source path
   * @param to - Destination path
   */
  async copyIfNotExists(from: string, to: string): Promise<void> {
    await wrapGrpcCall(
      "StorageService",
      "CopyIfNotExists",
      async () => {
        await this.client.copyIfNotExists({
          bindingName: this.bindingName,
          fromPath: from,
          toPath: to,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Rename (move) an object only if the destination doesn't exist.
   *
   * @param from - Source path
   * @param to - Destination path
   */
  async renameIfNotExists(from: string, to: string): Promise<void> {
    await wrapGrpcCall(
      "StorageService",
      "RenameIfNotExists",
      async () => {
        await this.client.renameIfNotExists({
          bindingName: this.bindingName,
          fromPath: from,
          toPath: to,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Generate a signed URL for an object.
   *
   * @param path - Path to the object
   * @param options - Signed URL options
   * @returns Signed URL result
   */
  async signedUrl(path: string, options: SignedUrlOptions): Promise<SignedUrlResult> {
    const operationMap: Record<SignedUrlOptions["operation"], StorageHttpMethod> = {
      get: StorageHttpMethod.HTTP_METHOD_GET,
      put: StorageHttpMethod.HTTP_METHOD_PUT,
      delete: StorageHttpMethod.HTTP_METHOD_DELETE,
    }

    // Calculate expiration time
    const expiresInSeconds = options.expiresInSeconds ?? 3600
    const expirationTime = new Date(Date.now() + expiresInSeconds * 1000)

    return await wrapGrpcCall(
      "StorageService",
      "SignedUrl",
      async () => {
        const response = await this.client.signedUrl({
          bindingName: this.bindingName,
          path,
          httpMethod: operationMap[options.operation],
          expirationTime,
        })
        return {
          url: response.url,
          expiresAt: expirationTime,
        }
      },
      { bindingName: this.bindingName },
    )
  }

  // Private helpers

  private fromProtoMeta(proto: ProtoObjectMeta): StorageObjectMeta {
    return {
      location: proto.location,
      lastModified: proto.lastModified,
      size: proto.size,
      etag: proto.eTag,
      version: proto.version,
    }
  }
}
