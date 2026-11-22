// The customer's "internal API": a small orders service on a private address, outside the
// stack. It has no idea sandboxes exist; only the connector calls it.
//
//   GET   /orders          list orders
//   GET   /orders/:id      one order
//   PATCH /orders/:id      { "status": "..." } changes an order's status
//
// Plain Node, no dependencies, so the same file runs in the local container and on EC2.

import { createServer } from "node:http"

const orders = new Map(
  [
    { id: "1001", customer: "Northwind Books", item: "Standing desk", total: 489.0, status: "shipped" },
    { id: "1002", customer: "Blue Fern Cafe", item: "Espresso grinder", total: 129.5, status: "processing" },
    { id: "1003", customer: "Harbor Dental", item: "Label printer", total: 214.99, status: "delivered" },
    { id: "1004", customer: "Quarry Outfitters", item: "Tent, 4 person", total: 349.0, status: "processing" },
  ].map(order => [order.id, order]),
)

const STATUSES = new Set(["processing", "shipped", "delivered", "cancelled", "refunded"])

function send(res, status, body) {
  res.writeHead(status, { "content-type": "application/json" })
  res.end(JSON.stringify(body))
}

async function readJson(req) {
  let raw = ""
  for await (const chunk of req) raw += chunk
  try {
    return JSON.parse(raw || "{}")
  } catch {
    return null
  }
}

createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", "http://internal")
  console.log(`${new Date().toISOString()} ${req.method} ${url.pathname}`)
  if (url.pathname === "/health") return send(res, 200, { ok: true })
  if (req.method === "GET" && url.pathname === "/orders") {
    return send(res, 200, { orders: [...orders.values()] })
  }
  const match = url.pathname.match(/^\/orders\/([\w-]+)$/)
  if (!match) return send(res, 404, { error: "not found" })
  const order = orders.get(match[1])
  if (!order) return send(res, 404, { error: `no order ${match[1]}` })
  if (req.method === "GET") return send(res, 200, order)
  if (req.method === "PATCH") {
    const body = await readJson(req)
    if (!body || !STATUSES.has(body.status)) {
      return send(res, 400, { error: `status must be one of ${[...STATUSES].join(", ")}` })
    }
    order.status = body.status
    return send(res, 200, order)
  }
  send(res, 405, { error: "method not allowed" })
}).listen(Number(process.env.PORT ?? 8080), "0.0.0.0", () => {
  console.log(`internal API listening on ${process.env.PORT ?? 8080}`)
})
