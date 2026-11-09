import * as alien from "@alienplatform/core"

const orders = new alien.Queue("orders").build()
const refunds = new alien.Queue("refunds").build()
const imports = new alien.Storage("imports").build()

const orderHandler = new alien.Worker("order-handler")
  .code({ type: "image", image: "ghcr.io/acme/orders:1.0.0" })
  .trigger({ type: "queue", queue: orders.ref() })
  .permissions("order-handler")
  .build()

const refundBatcher = new alien.Worker("refund-batcher")
  .code({ type: "image", image: "ghcr.io/acme/refunds:1.0.0" })
  .trigger({
    type: "queue",
    queue: refunds.ref(),
    batch: { maxMessages: 100, maxWindowSeconds: 120 },
  })
  .permissions("refund-batcher")
  .build()

const importReader = new alien.Worker("import-reader")
  .code({ type: "image", image: "ghcr.io/acme/imports:1.0.0" })
  .trigger({ type: "storage", storage: imports.ref(), events: ["created"] })
  .permissions("import-reader")
  .build()

const importAuditor = new alien.Worker("import-auditor")
  .code({ type: "image", image: "ghcr.io/acme/imports:1.0.0" })
  .trigger({ type: "storage", storage: imports.ref(), events: ["created"] })
  .permissions("import-auditor")
  .build()

const nightly = new alien.Worker("nightly")
  .code({ type: "image", image: "ghcr.io/acme/nightly:1.0.0" })
  .trigger({ type: "schedule", cron: "0 2 * * *" })
  .permissions("nightly")
  .build()

const everything = new alien.Worker("everything")
  .code({ type: "image", image: "ghcr.io/acme/everything:1.0.0" })
  .trigger({ type: "schedule", cron: "*/15 * * * *" })
  .trigger({ type: "storage", storage: imports.ref(), events: ["created"] })
  .trigger({ type: "queue", queue: orders.ref() })
  .trigger({
    type: "queue",
    queue: refunds.ref(),
    batch: { maxMessages: 5, maxWindowSeconds: 10 },
  })
  .permissions("everything")
  .build()

export default new alien.Stack("acme-triggers")
  .platforms(["aws", "gcp", "azure"])
  .add(orders, "frozen")
  .add(refunds, "frozen")
  .add(imports, "frozen")
  .add(orderHandler, "live")
  .add(refundBatcher, "live")
  .add(importReader, "live")
  .add(importAuditor, "live")
  .add(nightly, "live")
  .add(everything, "live")
  .permissions({
    profiles: {
      "order-handler": {},
      "refund-batcher": {},
      "import-reader": {},
      "import-auditor": {},
      nightly: {},
      everything: {},
    },
  })
  .build()
