/**
 * Minimal in-test HTTP server built on `node:http` so the exact same helper
 * runs under BOTH `vitest run` (Node) and `bun test` (Bun implements
 * `node:http`). It records every request (method, path, headers, parsed JSON
 * body) and delegates the response to a route callback.
 *
 * Deliberately dependency-free: no `undici`, no framework — the sender talks to
 * it over the platform global `fetch`, exactly as it would to the real command
 * server.
 */

import { createServer } from "node:http"

export interface CapturedRequest {
  method: string
  path: string
  headers: Record<string, string | string[] | undefined>
  body: unknown
}

export interface RouteResult {
  status?: number
  json?: unknown
  text?: string
}

export type RouteHandler = (req: CapturedRequest) => RouteResult | Promise<RouteResult>

export interface StubServer {
  baseUrl: string
  requests: CapturedRequest[]
  close: () => Promise<void>
}

export async function startStubServer(route: RouteHandler): Promise<StubServer> {
  const requests: CapturedRequest[] = []

  const server = createServer((req, res) => {
    const chunks: Buffer[] = []
    req.on("data", chunk => chunks.push(chunk as Buffer))
    req.on("end", async () => {
      const raw = Buffer.concat(chunks).toString("utf-8")
      let body: unknown
      try {
        body = raw ? JSON.parse(raw) : undefined
      } catch {
        body = raw
      }

      const captured: CapturedRequest = {
        method: req.method ?? "",
        path: req.url ?? "",
        headers: req.headers,
        body,
      }
      requests.push(captured)

      try {
        const result = await route(captured)
        const status = result.status ?? 200
        if (result.json !== undefined) {
          res.writeHead(status, { "content-type": "application/json" })
          res.end(JSON.stringify(result.json))
        } else if (result.text !== undefined) {
          res.writeHead(status, { "content-type": "text/plain" })
          res.end(result.text)
        } else {
          res.writeHead(status)
          res.end()
        }
      } catch (error) {
        res.writeHead(500)
        res.end(String(error))
      }
    })
  })

  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", () => resolve()))

  const address = server.address()
  const port = typeof address === "object" && address !== null ? address.port : 0

  return {
    baseUrl: `http://127.0.0.1:${port}`,
    requests,
    close: () =>
      new Promise<void>((resolve, reject) =>
        server.close(error => (error ? reject(error) : resolve())),
      ),
  }
}

/** Base64-encode a value the way the wire protocol expects (JSON → base64). */
export function encodeInlineJson(value: unknown): string {
  return Buffer.from(JSON.stringify(value), "utf-8").toString("base64")
}
