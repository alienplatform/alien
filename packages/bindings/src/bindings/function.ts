/**
 * Function binding implementation.
 *
 * Provides direct function-to-function invocation.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  FunctionServiceDefinition,
  type FunctionServiceClient as GeneratedClient,
} from "../generated/function.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type { FunctionInvokeRequest, FunctionInvokeResponse } from "../types.js"

/**
 * Function binding for direct function invocation.
 *
 * @example
 * ```typescript
 * import { func } from "@alienplatform/bindings"
 *
 * const processor = func("image-processor")
 *
 * // Invoke with JSON body
 * const response = await processor.invokeJson("resize-image", {
 *   imageUrl: "https://...",
 *   width: 800,
 *   height: 600,
 * })
 *
 * // Get function URL
 * const url = await processor.getUrl()
 * ```
 */
export class FunctionBinding {
  private readonly client: GeneratedClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(FunctionServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Invoke a function with raw request data.
   *
   * @param request - Invocation request
   * @returns Function response
   */
  async invoke(request: FunctionInvokeRequest): Promise<FunctionInvokeResponse> {
    return await wrapGrpcCall(
      "FunctionService",
      "Invoke",
      async () => {
        const response = await this.client.invoke({
          bindingName: this.bindingName,
          targetFunction: request.targetFunction,
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
   * Invoke a function with a JSON body.
   *
   * @param targetFunction - Target function identifier
   * @param body - JSON body
   * @param options - Optional request options
   * @returns Parsed JSON response
   */
  async invokeJson<TRequest, TResponse = unknown>(
    targetFunction: string,
    body: TRequest,
    options?: {
      method?: string
      path?: string
      headers?: Record<string, string>
      timeoutMs?: number
    },
  ): Promise<TResponse> {
    const response = await this.invoke({
      targetFunction,
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
      throw new Error(`Function invocation failed with status ${response.status}: ${errorText}`)
    }

    const responseText = new TextDecoder().decode(response.body)
    return JSON.parse(responseText) as TResponse
  }

  /**
   * Invoke a function with GET request.
   *
   * @param targetFunction - Target function identifier
   * @param path - Request path
   * @param options - Optional request options
   * @returns Parsed JSON response
   */
  async get<TResponse = unknown>(
    targetFunction: string,
    path = "/",
    options?: {
      headers?: Record<string, string>
      timeoutMs?: number
    },
  ): Promise<TResponse> {
    const response = await this.invoke({
      targetFunction,
      method: "GET",
      path,
      headers: options?.headers,
      timeoutMs: options?.timeoutMs,
    })

    if (response.status >= 400) {
      const errorText = new TextDecoder().decode(response.body)
      throw new Error(`Function GET failed with status ${response.status}: ${errorText}`)
    }

    const responseText = new TextDecoder().decode(response.body)
    return JSON.parse(responseText) as TResponse
  }

  /**
   * Get the public URL of the function.
   *
   * @returns Function URL if available
   */
  async getUrl(): Promise<string | undefined> {
    return await wrapGrpcCall(
      "FunctionService",
      "GetFunctionUrl",
      async () => {
        const response = await this.client.getFunctionUrl({
          bindingName: this.bindingName,
        })
        return response.url
      },
      { bindingName: this.bindingName },
    )
  }
}
