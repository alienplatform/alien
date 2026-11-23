// The customer's "scanner": a vulnerability-findings style service on a private address, outside
// the stack. It wants `Authorization: Bearer <key>`, a key the customer owns. Only the connector
// holds it.
//
//   GET /findings          list findings
//   GET /findings/:id      one finding
//
// Plain Node, no dependencies, so the same file runs in the local container and on EC2.

import { createServer } from "node:http"

const KEY = process.env.SCANNER_API_KEY
if (!KEY) {
  console.error("SCANNER_API_KEY is not set")
  process.exit(1)
}

const findings = [
  { id: "F-201", host: "web-01", severity: "high", title: "OpenSSL needs a patch" },
  { id: "F-202", host: "web-02", severity: "medium", title: "TLS 1.0 still enabled" },
  { id: "F-203", host: "db-01", severity: "low", title: "Default banner exposes the version" },
]

function send(res, status, body) {
  res.writeHead(status, { "content-type": "application/json" })
  res.end(JSON.stringify(body))
}

createServer((req, res) => {
  const url = new URL(req.url ?? "/", "http://scanner")
  const authorized = req.headers.authorization === `Bearer ${KEY}`
  console.log(`${new Date().toISOString()} ${req.method} ${url.pathname} ${authorized ? "ok" : "refused"}`)
  if (url.pathname === "/health") return send(res, 200, { ok: true })
  if (!authorized) return send(res, 401, { error: "invalid scanner key" })
  if (req.method !== "GET") return send(res, 405, { error: "method not allowed" })
  if (url.pathname === "/findings") return send(res, 200, { findings })
  const match = url.pathname.match(/^\/findings\/([\w-]+)$/)
  const finding = match && findings.find(f => f.id === match[1])
  if (!finding) return send(res, 404, { error: "not found" })
  send(res, 200, finding)
}).listen(Number(process.env.PORT ?? 8081), "0.0.0.0", () => {
  console.log(`scanner listening on ${process.env.PORT ?? 8081}`)
})
