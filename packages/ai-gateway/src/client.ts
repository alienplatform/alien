/**
 * AI binding — a thin OpenAI-compatible client to the workload's AI gateway.
 *
 * For an ambient-cloud binding the in-process Rust gateway is started on the first call; for a
 * BYO-key (External) binding the client talks to the provider directly. Either way the client
 * only forwards OpenAI-shaped requests and streams responses back — it holds no ambient
 * credentials and rewrites nothing (the gateway rewrites the model id and injects the
 * credential).
 */

import { AlienError } from "@alienplatform/core"

import { isExternalAiBinding, parseAiBinding } from "./binding.js"
import { AiTransportError, AiUpstreamError, BindingNotFoundError } from "./errors.js"
import type { Gateway } from "./gateway.js"

// ─────────────────────────────────────────────────────────────────────────────
// Public request / response types
// ─────────────────────────────────────────────────────────────────────────────

export interface ChatCompletionCreateParams {
  model: string
  messages: Array<{ role: string; content: string | object }>
  stream?: boolean
  [key: string]: unknown
}

export interface ResponseCreateParams {
  model: string
  input: string | Array<{ role: string; content: string }>
  stream?: boolean
  [key: string]: unknown
}

/** One model the gateway exposes for this binding's cloud. */
export interface AiModel {
  id: string
}

// Upstream base URL (no `/v1`) for a BYO-key provider. `ALIEN_AI_LOCAL_BASE_URL` overrides it so
// any OpenAI-compatible provider works; unknown providers default to OpenAI's.
function providerBaseUrl(provider: string): string {
  const override = process.env.ALIEN_AI_LOCAL_BASE_URL
  if (override) return override.replace(/\/$/, "")
  return provider === "anthropic" ? "https://api.anthropic.com" : "https://api.openai.com"
}

// A small curated chat-model list for the BYO-key picker (we don't proxy the provider's own
// /v1/models, which returns hundreds of non-chat entries).
function defaultModels(provider: string): string[] {
  return provider === "anthropic"
    ? ["claude-3-5-sonnet-latest", "claude-3-5-haiku-latest"]
    : ["gpt-4o-mini", "gpt-4o"]
}

/** Resolution shared by `ai()` and `getAiConnection()`. `baseUrl` is the root (no `/v1`);
 * `apiKey`/`staticModels` are set only for a BYO-key (External) provider. */
export interface ResolvedAiBinding {
  baseUrl: string
  apiKey?: string
  staticModels?: string[]
}

/**
 * Resolve `ALIEN_<NAME>_BINDING` to a connection target. External -> the provider directly with
 * the projected key. Any other service tag (the ambient variants, including ones this SDK
 * predates) -> the in-process Rust gateway, started here on demand, which validates the binding
 * and injects the ambient cloud credential. Backs both `ai()` and `getAiConnection()`.
 *
 * The returned `baseUrl` is the root without `/v1` — the gateway serves
 * `/<segment>/v1/...`, and the client appends the versioned paths itself.
 */
export async function resolveAiBinding(gateway: Gateway, name: string): Promise<ResolvedAiBinding> {
  const binding = await parseAiBinding(name)
  if (!binding) {
    throw new AlienError(BindingNotFoundError.create({ bindingName: name, bindingType: "Ai" }))
  }
  if (isExternalAiBinding(binding)) {
    return {
      baseUrl: providerBaseUrl(binding.provider),
      apiKey: binding.apiKey,
      staticModels: defaultModels(binding.provider),
    }
  }
  const handle = await gateway.startAiGateway()
  // Mirror the gateway's `canonical_binding_name` (lowercase, `_`->`-`) so the route key
  // matches for every legal resource id, including underscored ones.
  const segment = name.toLowerCase().replace(/_/g, "-")
  return { baseUrl: `${handle.url}/${segment}` }
}

// ─────────────────────────────────────────────────────────────────────────────
// Upstream error constructor
//
// 429/502/503/504 are transient and safe to retry; all others are not. The gateway
// URL is loopback (not sensitive), but the error stays internal=true because the
// forwarded provider message may carry upstream detail.
// ─────────────────────────────────────────────────────────────────────────────

const RETRYABLE_STATUSES = new Set([429, 502, 503, 504])

function createUpstreamError(url: string, status: number, message: string): AlienError {
  return new AlienError({
    ...AiUpstreamError.create({ url, status, message }).toOptions(),
    // Retryability and the surfaced status vary per HTTP status, not by the
    // definition, so override just those two on the schema-typed base.
    retryable: RETRYABLE_STATUSES.has(status),
    httpStatusCode: status >= 400 ? status : 502,
  })
}

// ─────────────────────────────────────────────────────────────────────────────
// SSE streaming parser
//
// Reads from a Fetch API ReadableStream and yields each `data:` JSON payload,
// stopping at `data: [DONE]`. Chunks pass through unchanged — the gateway already
// returns the upstream's native stream. Uses getReader() rather than `for await`
// because web ReadableStream is not reliably async-iterable in all runtimes (Bun).
// ─────────────────────────────────────────────────────────────────────────────

async function* parseSse(
  url: string,
  body: ReadableStream<Uint8Array>,
): AsyncGenerator<Record<string, unknown>> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ""

  const emit = function* (line: string): Generator<Record<string, unknown>> {
    const trimmed = line.trimEnd()
    if (!trimmed.startsWith("data:")) return
    const payload = trimmed.slice(5).trim()
    if (payload === "[DONE]") return
    let chunk: Record<string, unknown>
    try {
      chunk = JSON.parse(payload) as Record<string, unknown>
    } catch (cause) {
      throw new AlienError(
        AiTransportError.create({
          url,
          reason: `Malformed SSE chunk: ${cause instanceof Error ? cause.message : String(cause)}`,
        }),
      )
    }
    yield chunk
  }

  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split("\n")
      buffer = lines.pop() ?? ""

      for (const line of lines) {
        if (line.trimEnd() === "data: [DONE]") return
        yield* emit(line)
      }
    }

    // Flush any bytes still held by the decoder, then any trailing line.
    buffer += decoder.decode()
    for (const line of buffer.split("\n")) {
      if (line.trimEnd() === "data: [DONE]") return
      yield* emit(line)
    }
  } finally {
    // cancel() both cancels the stream and releases the reader lock.
    await reader.cancel().catch(() => {})
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Ai class
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Constructed by the `ai(name)` factory; resolves the binding (and starts the
 * gateway, for ambient) once on first use.
 *
 * @example
 * ```typescript
 * import { ai } from "@alienplatform/sdk"
 *
 * const llm = ai("my-llm")
 * const response = await llm.chat.completions.create({
 *   model: "claude-opus-4.8",
 *   messages: [{ role: "user", content: "Hello!" }],
 * })
 * ```
 */
export class Ai {
  private readonly resolve: () => Promise<ResolvedAiBinding>
  private connectionPromise: Promise<ResolvedAiBinding> | null = null

  /** OpenAI-compatible chat namespace. */
  readonly chat: {
    completions: {
      create(params: ChatCompletionCreateParams): Promise<unknown>
    }
  }

  /** OpenAI-compatible Responses API namespace (the surface Codex speaks). */
  readonly responses: {
    create(params: ResponseCreateParams): Promise<unknown>
  }

  constructor(resolve: () => Promise<ResolvedAiBinding>) {
    this.resolve = resolve

    this.chat = {
      completions: {
        create: (params: ChatCompletionCreateParams) => this._chatCompletionsCreate(params),
      },
    }

    this.responses = {
      create: (params: ResponseCreateParams) => this._responsesCreate(params),
    }
  }

  // Resolve the binding (and start the gateway for an ambient one) once, then reuse.
  // Only a resolved connection is memoized: caching a rejection would leave a retained
  // instance permanently broken after one transient gateway-start failure, which the Rust
  // side reports as retryable — the same guarantee `createGateway` keeps.
  private connection(): Promise<ResolvedAiBinding> {
    this.connectionPromise ??= this.resolve().catch(error => {
      this.connectionPromise = null
      throw error
    })
    return this.connectionPromise
  }

  /** List the models the gateway exposes for this binding's cloud. */
  async getAvailableModels(): Promise<AiModel[]> {
    const { baseUrl, staticModels } = await this.connection()
    // Curated default for a BYO-key provider (see `defaultModels`); the gateway
    // path fetches the cloud's catalog below.
    if (staticModels) {
      return staticModels.map(id => ({ id }))
    }
    const url = `${baseUrl}/v1/models`
    const response = await this._fetch(url, { method: "GET" })
    if (!response.ok) {
      throw createUpstreamError(url, response.status, await extractErrorMessage(response))
    }
    let body: { data?: AiModel[] }
    try {
      body = (await response.json()) as { data?: AiModel[] }
    } catch (jsonError) {
      throw (await AlienError.from(jsonError)).withContext(
        AiTransportError.create({
          url,
          reason: `Response body is not valid JSON: ${jsonError instanceof Error ? jsonError.message : String(jsonError)}`,
        }),
      )
    }
    // The gateway always returns a `data` array; its absence means a broken response.
    if (!Array.isArray(body.data)) {
      throw createUpstreamError(url, response.status, "models response had no data array")
    }
    return body.data
  }

  private _chatCompletionsCreate(params: ChatCompletionCreateParams): Promise<unknown> {
    return this._postSurface("/v1/chat/completions", params)
  }

  private _responsesCreate(params: ResponseCreateParams): Promise<unknown> {
    return this._postSurface("/v1/responses", params)
  }

  // The chat-completions and responses surfaces are byte-for-byte passthroughs that differ
  // only in path, so they share one POST + stream/JSON handler. The gateway (or provider)
  // returns the upstream's native body unchanged.
  private async _postSurface(
    path: string,
    params: { stream?: boolean } & Record<string, unknown>,
  ): Promise<unknown> {
    const { baseUrl, apiKey } = await this.connection()
    const url = `${baseUrl}${path}`
    const response = await this._fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        // Only the BYO-key path authenticates here; ambient credentials are the gateway's.
        ...(apiKey ? { Authorization: `Bearer ${apiKey}` } : {}),
      },
      body: JSON.stringify(params),
    })

    if (!response.ok) {
      throw createUpstreamError(url, response.status, await extractErrorMessage(response))
    }

    if (params.stream === true) {
      if (!response.body) {
        throw createUpstreamError(url, response.status, "Streaming response body is null")
      }
      return parseSse(url, response.body)
    }

    try {
      return await response.json()
    } catch (jsonError) {
      throw (await AlienError.from(jsonError)).withContext(
        AiTransportError.create({
          url,
          reason: `Response body is not valid JSON: ${jsonError instanceof Error ? jsonError.message : String(jsonError)}`,
        }),
      )
    }
  }

  private async _fetch(url: string, init: RequestInit): Promise<Response> {
    try {
      return await fetch(url, init)
    } catch (fetchError) {
      throw (await AlienError.from(fetchError)).withContext(
        AiTransportError.create({
          url,
          reason: fetchError instanceof Error ? fetchError.message : String(fetchError),
        }),
      )
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry wiring
// ─────────────────────────────────────────────────────────────────────────────

/**
 * A resolved AI connection for a plain OpenAI-compatible client: the base URL, plus an
 * optional bearer key. `apiKey` is set for a BYO-key (External) provider and omitted for
 * ambient-cloud bindings, where the embedded gateway injects the credential.
 */
export interface AiConnection {
  baseURL: string
  apiKey?: string
}

/** The app-facing AI client surface, shared by the lazy-loading and static-embed entries. */
export interface AiClient {
  ai(name: string): Ai
  getAiConnection(name: string): Promise<AiConnection>
}

/**
 * Build the `ai()` / `getAiConnection()` pair over a gateway. Mirrors `createGateway`:
 * each entry wires its own addon acquisition and shares this implementation.
 */
export function createAiClient(gateway: Gateway): AiClient {
  return {
    /** An OpenAI-compatible client for the named AI binding. */
    ai(name: string): Ai {
      return new Ai(() => resolveAiBinding(gateway, name))
    },

    /**
     * Resolve an AI binding to `{ baseURL, apiKey? }` for a plain OpenAI-compatible client
     * (e.g. the Vercel AI SDK's `createOpenAICompatible`). Mirrors `getPostgresConnection`:
     * the binding decides the target, so app code is identical whether AI is a BYO-key
     * provider (local) or an ambient-cloud model behind the gateway.
     *
     * Awaits gateway startup for an ambient binding, so the returned `baseURL` is live
     * before the caller's client uses it.
     */
    async getAiConnection(name: string): Promise<AiConnection> {
      const resolved = await resolveAiBinding(gateway, name)
      return { baseURL: `${resolved.baseUrl}/v1`, apiKey: resolved.apiKey }
    },
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

async function extractErrorMessage(response: Response): Promise<string> {
  try {
    const errBody = (await response.json()) as Record<string, unknown>
    const errObj = errBody.error
    if (errObj && typeof errObj === "object" && "message" in errObj) {
      return String((errObj as Record<string, unknown>).message)
    }
    return errBody.message ? String(errBody.message) : response.statusText
  } catch {
    return response.statusText || "Unknown upstream error"
  }
}
