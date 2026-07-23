import { AlienError } from "@alienplatform/core"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { createAiClient } from "../client.js"
import type { Gateway } from "../gateway.js"
import type { RawAiGatewayHandle } from "../loader.js"

// ─────────────────────────────────────────────────────────────────────────────
// Harness. The `Ai` client is exercised against a BYO-key (External) binding, where
// it talks to the provider directly and the HTTP behavior is mockable via global
// fetch. The ambient path is resolved through a stubbed Gateway (the real Rust
// addon is covered by the crate's own tests); here we only assert the client +
// binding resolution, including the URL seam (`<gateway>/<segment>/v1/...`).
// ─────────────────────────────────────────────────────────────────────────────

const EXTERNAL = JSON.stringify({ service: "external", provider: "openai", apiKey: "sk-test" })

const GATEWAY_URL = "http://127.0.0.1:41999"

const stubGateway: Gateway = {
  startAiGateway: () => Promise.resolve({ url: GATEWAY_URL } as unknown as RawAiGatewayHandle),
}

const { ai, getAiConnection } = createAiClient(stubGateway)

beforeEach(() => {
  vi.stubEnv("ALIEN_LLM_BINDING", EXTERNAL)
})

afterEach(() => {
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
})

function stubFetch(responseBody: unknown, status = 200): ReturnType<typeof vi.fn> {
  const fetchMock = vi.fn().mockResolvedValue({
    ok: status >= 200 && status < 300,
    status,
    statusText: status === 200 ? "OK" : "Error",
    json: vi.fn().mockResolvedValue(responseBody),
    text: vi.fn().mockResolvedValue(JSON.stringify(responseBody)),
  })
  vi.stubGlobal("fetch", fetchMock)
  return fetchMock
}

function buildSseBody(chunks: unknown[]): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder()
  const lines = [...chunks.map(c => `data: ${JSON.stringify(c)}\n\n`), "data: [DONE]\n\n"].join("")
  return new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encoder.encode(lines))
      controller.close()
    },
  })
}

function stubFetchSse(chunks: unknown[]): ReturnType<typeof vi.fn> {
  const fetchMock = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    statusText: "OK",
    body: buildSseBody(chunks),
    json: vi.fn().mockRejectedValue(new Error("not a JSON response")),
    text: vi.fn().mockRejectedValue(new Error("not a text response")),
  })
  vi.stubGlobal("fetch", fetchMock)
  return fetchMock
}

const callUrl = (m: ReturnType<typeof vi.fn>): string => m.mock.calls[0]![0] as string
const callInit = (m: ReturnType<typeof vi.fn>): RequestInit => m.mock.calls[0]![1] as RequestInit
const callBody = (m: ReturnType<typeof vi.fn>): Record<string, unknown> =>
  JSON.parse(callInit(m).body as string) as Record<string, unknown>

// ─────────────────────────────────────────────────────────────────────────────
// getAiConnection
// ─────────────────────────────────────────────────────────────────────────────

describe("getAiConnection", () => {
  it("resolves an External (BYO-key) binding to the provider directly, with the key", async () => {
    expect(await getAiConnection("llm")).toEqual({
      baseURL: "https://api.openai.com/v1",
      apiKey: "sk-test",
    })
  })

  it("resolves an ambient-cloud binding to the in-process gateway, with no key", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", JSON.stringify({ service: "bedrock", region: "us-east-2" }))
    const conn = await getAiConnection("llm")
    // The gateway serves `/<segment>/v1/...`; the connection carries exactly one `/v1`.
    expect(conn.baseURL).toBe(`${GATEWAY_URL}/llm/v1`)
    expect(conn.apiKey).toBeUndefined()
  })

  it("uses the resource id as the gateway route segment", async () => {
    vi.stubEnv("ALIEN_MY_LLM_BINDING", JSON.stringify({ service: "bedrock", region: "us-east-2" }))
    const conn = await getAiConnection("My-LLM")
    expect(conn.baseURL).toBe(`${GATEWAY_URL}/my-llm/v1`)
  })

  it("honors ALIEN_AI_LOCAL_BASE_URL to point at any OpenAI-compatible provider", async () => {
    vi.stubEnv("ALIEN_AI_LOCAL_BASE_URL", "http://localhost:11434")
    expect((await getAiConnection("llm")).baseURL).toBe("http://localhost:11434/v1")
  })

  it("throws BINDING_NOT_FOUND when the binding env var is missing", async () => {
    await expect(getAiConnection("unlinked")).rejects.toMatchObject({
      code: "BINDING_NOT_FOUND",
      httpStatusCode: 404,
    })
  })

  it("routes a service tag this SDK predates to the gateway (no client-side rejection)", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", JSON.stringify({ service: "sagemaker", endpoint: "x" }))
    const conn = await getAiConnection("llm")
    expect(conn.baseURL).toBe(`${GATEWAY_URL}/llm/v1`)
  })

  it("throws INVALID_BINDING_CONFIG on malformed JSON", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", "{not json")
    await expect(getAiConnection("llm")).rejects.toMatchObject({
      code: "INVALID_BINDING_CONFIG",
    })
  })

  it("rejects an external binding with an unexpected key (strict at the trust boundary)", async () => {
    vi.stubEnv(
      "ALIEN_LLM_BINDING",
      JSON.stringify({ service: "external", provider: "openai", apiKey: "sk", extra: "tampered" }),
    )
    await expect(getAiConnection("llm")).rejects.toMatchObject({
      code: "INVALID_BINDING_CONFIG",
    })
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// Ai client (against a BYO-key provider) — chat, streaming, models, errors
// ─────────────────────────────────────────────────────────────────────────────

describe("Ai.chat.completions.create (non-streaming)", () => {
  it("POSTs to the provider with the body unchanged and the BYO-key auth header", async () => {
    const fetchMock = stubFetch({ id: "x", model: "gpt-4o", choices: [] })
    const params = {
      model: "gpt-4o",
      messages: [{ role: "user", content: "hi" }],
      temperature: 0.5,
    }

    await ai("llm").chat.completions.create(params)

    expect(callUrl(fetchMock)).toBe("https://api.openai.com/v1/chat/completions")
    expect(callInit(fetchMock).method).toBe("POST")
    expect(callBody(fetchMock)).toEqual(params)
    const headers = (callInit(fetchMock).headers ?? {}) as Record<string, string>
    expect(headers.Authorization).toBe("Bearer sk-test")
  })

  it("canonicalizes the binding name to the ALIEN_<NAME>_BINDING env var", async () => {
    vi.stubEnv("ALIEN_MY_LLM_BINDING", EXTERNAL)
    const fetchMock = stubFetch({ id: "x", choices: [] })
    await ai("My-LLM").chat.completions.create({ model: "gpt-4o", messages: [] })
    expect(callUrl(fetchMock)).toBe("https://api.openai.com/v1/chat/completions")
  })

  it("POSTs an ambient binding through the gateway with a single /v1 and no key", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", JSON.stringify({ service: "bedrock", region: "us-east-2" }))
    const fetchMock = stubFetch({ id: "x", choices: [] })
    await ai("llm").chat.completions.create({ model: "gpt-oss-20b", messages: [] })
    expect(callUrl(fetchMock)).toBe(`${GATEWAY_URL}/llm/v1/chat/completions`)
    const headers = (callInit(fetchMock).headers ?? {}) as Record<string, string>
    expect(headers.Authorization).toBeUndefined()
  })

  it("throws an AiUpstreamError on a non-2xx response", async () => {
    stubFetch({ error: { message: "boom" } }, 500)
    await expect(
      ai("llm").chat.completions.create({ model: "gpt-4o", messages: [] }),
    ).rejects.toThrow(AlienError)
  })
})

describe("Ai.chat.completions.create (streaming)", () => {
  it("returns an async iterable of SSE chunks when stream: true", async () => {
    stubFetchSse([
      { choices: [{ delta: { content: "he" } }] },
      { choices: [{ delta: { content: "llo" } }] },
    ])
    const stream = (await ai("llm").chat.completions.create({
      model: "gpt-4o",
      messages: [],
      stream: true,
    })) as AsyncIterable<{ choices: { delta: { content?: string } }[] }>
    const parts: string[] = []
    for await (const chunk of stream) {
      const c = chunk.choices[0]?.delta.content
      if (c) parts.push(c)
    }
    expect(parts.join("")).toBe("hello")
  })
})

describe("Ai.getAvailableModels", () => {
  it("returns a curated default for a BYO-key provider without hitting it", async () => {
    const fetchMock = stubFetch({ data: [] })
    const models = await ai("llm").getAvailableModels()
    expect(models.map(m => m.id)).toContain("gpt-4o-mini")
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it("fetches the gateway's curated catalog for an ambient binding", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", JSON.stringify({ service: "bedrock", region: "us-east-2" }))
    const fetchMock = stubFetch({ data: [{ id: "gpt-oss-20b" }] })
    const models = await ai("llm").getAvailableModels()
    expect(callUrl(fetchMock)).toBe(`${GATEWAY_URL}/llm/v1/models`)
    expect(models).toEqual([{ id: "gpt-oss-20b" }])
  })

  it("retries a transient gateway-start failure on a retained instance", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", JSON.stringify({ service: "bedrock", region: "us-east-2" }))
    stubFetch({ data: [{ id: "gpt-oss-20b" }] })
    const start = vi
      .fn()
      .mockRejectedValueOnce(new Error("ambient credential unavailable"))
      .mockResolvedValue({ url: GATEWAY_URL } as unknown as RawAiGatewayHandle)
    const llm = createAiClient({ startAiGateway: start }).ai("llm")

    await expect(llm.getAvailableModels()).rejects.toThrow()
    // A cached rejection would leave this instance permanently broken; it must retry.
    expect(await llm.getAvailableModels()).toEqual([{ id: "gpt-oss-20b" }])
    expect(start).toHaveBeenCalledTimes(2)
  })
})

describe("Ai.responses.create", () => {
  it("POSTs to the provider's /v1/responses with the body unchanged and the BYO-key auth header", async () => {
    const fetchMock = stubFetch({ id: "resp_x", object: "response", output: [] })
    const params = { model: "gpt-4o", input: "hi" }
    await ai("llm").responses.create(params)
    expect(callUrl(fetchMock)).toBe("https://api.openai.com/v1/responses")
    expect(callInit(fetchMock).method).toBe("POST")
    expect(callBody(fetchMock)).toEqual(params)
    const headers = (callInit(fetchMock).headers ?? {}) as Record<string, string>
    expect(headers.Authorization).toBe("Bearer sk-test")
  })

  it("POSTs an ambient binding through the gateway with a single /v1 and no key", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", JSON.stringify({ service: "bedrock", region: "us-east-2" }))
    const fetchMock = stubFetch({ id: "resp_x", output: [] })
    await ai("llm").responses.create({ model: "gpt-oss-120b", input: "hi" })
    expect(callUrl(fetchMock)).toBe(`${GATEWAY_URL}/llm/v1/responses`)
    const headers = (callInit(fetchMock).headers ?? {}) as Record<string, string>
    expect(headers.Authorization).toBeUndefined()
  })

  it("returns an async iterable of SSE chunks when stream: true", async () => {
    stubFetchSse([
      { type: "response.output_text.delta", delta: "he" },
      { type: "response.output_text.delta", delta: "llo" },
    ])
    const stream = (await ai("llm").responses.create({
      model: "gpt-4o",
      input: "hi",
      stream: true,
    })) as AsyncIterable<{ type: string; delta?: string }>
    const parts: string[] = []
    for await (const chunk of stream) {
      if (chunk.delta) parts.push(chunk.delta)
    }
    expect(parts.join("")).toBe("hello")
  })

  it("throws an AiUpstreamError on a non-2xx response", async () => {
    stubFetch({ error: { message: "boom" } }, 500)
    await expect(ai("llm").responses.create({ model: "gpt-4o", input: "hi" })).rejects.toThrow(
      AlienError,
    )
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// Ai.finetune / Ai.finetuneStatus — runtime fine-tuning (ambient gateway only)
// ─────────────────────────────────────────────────────────────────────────────

const AMBIENT = JSON.stringify({ service: "bedrock", region: "us-east-2" })

describe("Ai.finetune", () => {
  it("POSTs the training key to the gateway and returns { jobId, servedModel }", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", AMBIENT)
    const fetchMock = stubFetch({ jobId: "job-123", servedModel: "llm-tuned" })

    const result = await ai("llm").finetune({ trainingKey: "datasets/train.jsonl" })

    expect(callUrl(fetchMock)).toBe(`${GATEWAY_URL}/llm/v1/finetune`)
    expect(callInit(fetchMock).method).toBe("POST")
    expect(callBody(fetchMock)).toEqual({ trainingKey: "datasets/train.jsonl" })
    // Ambient path injects the credential in the gateway; no client-side auth header.
    const headers = (callInit(fetchMock).headers ?? {}) as Record<string, string>
    expect(headers.Authorization).toBeUndefined()
    expect(result).toEqual({ jobId: "job-123", servedModel: "llm-tuned" })
  })

  it("omits trainingKey from the body when not provided", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", AMBIENT)
    const fetchMock = stubFetch({ jobId: "job-1", servedModel: "llm-tuned" })

    await ai("llm").finetune()

    expect(callBody(fetchMock)).toEqual({})
  })

  it("rejects for a BYO-key External binding without POSTing", async () => {
    // EXTERNAL binding is stubbed in the top-level beforeEach.
    const fetchMock = stubFetch({ jobId: "x", servedModel: "y" })
    await expect(ai("llm").finetune({ trainingKey: "k" })).rejects.toMatchObject({
      code: "INVALID_BINDING_CONFIG",
    })
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it("throws an AiUpstreamError on a non-2xx gateway response", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", AMBIENT)
    stubFetch({ error: { message: "boom" } }, 500)
    await expect(ai("llm").finetune({ trainingKey: "k" })).rejects.toThrow(AlienError)
  })
})

describe("Ai.finetuneStatus", () => {
  it("GETs the job by id and maps a succeeded status with the tuned model", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", AMBIENT)
    const fetchMock = stubFetch({ status: "succeeded", model: "llm-tuned-v1" })

    const status = await ai("llm").finetuneStatus("job-123")

    expect(callUrl(fetchMock)).toBe(`${GATEWAY_URL}/llm/v1/finetune/job-123`)
    expect(callInit(fetchMock).method).toBe("GET")
    expect(status).toEqual({ status: "succeeded", model: "llm-tuned-v1" })
  })

  it("maps a running status", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", AMBIENT)
    stubFetch({ status: "running" })
    expect(await ai("llm").finetuneStatus("job-1")).toEqual({ status: "running" })
  })

  it("URL-encodes the job id", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", AMBIENT)
    const fetchMock = stubFetch({ status: "running" })
    await ai("llm").finetuneStatus("arn:aws:bedrock/job 1")
    expect(callUrl(fetchMock)).toBe(
      `${GATEWAY_URL}/llm/v1/finetune/${encodeURIComponent("arn:aws:bedrock/job 1")}`,
    )
  })

  it("rejects for a BYO-key External binding without a GET", async () => {
    const fetchMock = stubFetch({ status: "running" })
    await expect(ai("llm").finetuneStatus("job-1")).rejects.toMatchObject({
      code: "INVALID_BINDING_CONFIG",
    })
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it("throws an AiUpstreamError on a non-2xx gateway response", async () => {
    vi.stubEnv("ALIEN_LLM_BINDING", AMBIENT)
    stubFetch({ error: { message: "not found" } }, 404)
    await expect(ai("llm").finetuneStatus("missing")).rejects.toThrow(AlienError)
  })
})
