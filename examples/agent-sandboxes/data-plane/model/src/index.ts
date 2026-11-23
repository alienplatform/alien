// Model: serves the Anthropic Messages API to the sandboxes on :8080, backed by the stack's AI
// binding. On AWS the binding is Bedrock, called with this container's own identity, so no
// model key exists anywhere: the sandbox sends `x-api-key: placeholder`, and it is dropped here.
//
// Sandboxes reach this container only through their link.

import { createServer, type IncomingMessage } from "node:http"
import { getAiConnection } from "@alienplatform/sdk"

// Headers the sandbox sends that must not reach the model service.
const DROPPED = new Set(["host", "authorization", "x-api-key", "content-length", "connection"])

function forwardHeaders(req: IncomingMessage, apiKey?: string): Headers {
  const headers = new Headers()
  for (const [name, value] of Object.entries(req.headers)) {
    if (typeof value === "string" && !DROPPED.has(name)) headers.set(name, value)
  }
  if (apiKey) {
    headers.set("x-api-key", apiKey)
    headers.set("authorization", `Bearer ${apiKey}`)
  }
  return headers
}

createServer(async (req, res) => {
  const path = (req.url ?? "/").split("?")[0]
  if (path === "/health") {
    res.writeHead(200).end("ok")
    return
  }
  if (!path.startsWith("/v1/")) {
    res.writeHead(404, { "content-type": "application/json" }).end('{"error":"not found"}')
    return
  }
  try {
    // Resolved per request: on a cloud this also starts the gateway on first use.
    const { baseURL, apiKey } = await getAiConnection("llm")
    let body = ""
    for await (const chunk of req) body += chunk
    const upstream = await fetch(`${baseURL}${path.slice("/v1".length)}`, {
      method: req.method,
      headers: forwardHeaders(req, apiKey),
      body: req.method === "GET" || req.method === "HEAD" ? undefined : body,
    })
    const headers: Record<string, string> = {}
    upstream.headers.forEach((value, name) => {
      if (name !== "content-encoding" && name !== "content-length") headers[name] = value
    })
    res.writeHead(upstream.status, headers)
    // Streamed as it arrives: Claude Code reads the model's answer as server-sent events.
    if (upstream.body) {
      const reader = upstream.body.getReader()
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        res.write(value)
      }
    }
    res.end()
    console.log(`${req.method} ${path} -> ${upstream.status}`)
  } catch (error) {
    console.error("model call failed", error)
    if (!res.headersSent) res.writeHead(502, { "content-type": "application/json" })
    res.end('{"type":"error","error":{"type":"api_error","message":"model service unreachable"}}')
  }
}).listen(Number(process.env.PORT ?? 8080), "0.0.0.0", () => {
  console.log(`model listening on ${process.env.PORT ?? 8080}`)
})
