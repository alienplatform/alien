/**
 * Worker binding implementation.
 *
 * Provides direct worker-to-worker invocation.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type WorkerServiceClient as GeneratedClient,
  WorkerServiceDefinition,
} from "../generated/worker.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type { WorkerInvokeRequest, WorkerInvokeResponse } from "../types.js"

/**
 * Worker binding for direct worker invocation.
 *
 * @example
 * ```typescript
 * import { worker } from "@alienplatform/sdk"
 *
 * const processor = worker("image-processor")
 *
 * // Invoke with JSON body
 * const response = await processor.invokeJson("resize-image", {
 *   imageUrl: "https://...",
 *   width: 800,
 *   height: 600,
 * })
 *
 * // Get worker URL
 * const url = await processor.getUrl()
 * ```
 */
export class WorkerBinding {
  private readonly client: GeneratedClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(WorkerServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Invoke a worker with raw request data.
   *
   * @param request - Invocation request
   * @returns Worker response
   */
  async invoke(request: WorkerInvokeRequest): Promise<WorkerInvokeResponse> {
    return await wrapGrpcCall(
      "WorkerService",
      "Invoke",
      async () => {
        const response = await this.client.invoke({
          bindingName: this.bindingName,
          targetWorker: request.targetWorker,
          method: request.method,
          path: request.path,
          headers: request.headers ?? {},
          body: request.body ?? new Uint8Array(),
          timeoutSeconds: request.timeoutMs ? Math.floor(request.timeoutMs / 1000) : undefined,
        })
        return {
          status: response.status,
          headers: response.headers,
          body: response.body,
        }
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Invoke a worker with a JSON body.
   *
   * @param targetWorker - Target worker identifier
   * @param body - JSON body
   * @param options - Optional request options
   * @returns Parsed JSON response
   */
  async invokeJson<TRequest, TResponse = unknown>(
    targetWorker: string,
    body: TRequest,
    options?: {
      method?: string
      path?: string
      headers?: Record<string, string>
      timeoutMs?: number
    },
  ): Promise<TResponse> {
    const response = await this.invoke({
      targetWorker,
      method: options?.method ?? "POST",
      path: options?.path ?? "/",
      headers: {
        "content-type": "application/json",
        ...options?.headers,
      },
      body: new TextEncoder().encode(JSON.stringify(body)),
      timeoutMs: options?.timeoutMs,
    })

    if (response.status >= 400) {
      const errorText = new TextDecoder().decode(response.body)
      throw new Error(`Worker invocation failed with status ${response.status}: ${errorText}`)
    }

    const responseText = new TextDecoder().decode(response.body)
    return JSON.parse(responseText) as TResponse
  }

  /**
   * Invoke a worker with GET request.
   *
   * @param targetWorker - Target worker identifier
   * @param path - Request path
   * @param options - Optional request options
   * @returns Parsed JSON response
   */
  async get<TResponse = unknown>(
    targetWorker: string,
    path = "/",
    options?: {
      headers?: Record<string, string>
      timeoutMs?: number
    },
  ): Promise<TResponse> {
    const response = await this.invoke({
      targetWorker,
      method: "GET",
      path,
      headers: options?.headers,
      timeoutMs: options?.timeoutMs,
    })

    if (response.status >= 400) {
      const errorText = new TextDecoder().decode(response.body)
      throw new Error(`Worker GET failed with status ${response.status}: ${errorText}`)
    }

    const responseText = new TextDecoder().decode(response.body)
    return JSON.parse(responseText) as TResponse
  }

  /**
   * Get the public URL of the worker.
   *
   * @returns Worker URL if available
   */
  async getUrl(): Promise<string | undefined> {
    return await wrapGrpcCall(
      "WorkerService",
      "GetWorkerUrl",
      async () => {
        const response = await this.client.getWorkerUrl({
          bindingName: this.bindingName,
        })
        return response.url
      },
      { bindingName: this.bindingName },
    )
  }
}
