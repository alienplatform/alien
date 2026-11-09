// The alien.ts equivalent of alien.toml. Both resolve to the same stack.
import * as alien from "@alienplatform/core"

const sessions = new alien.Kv("sessions").build()
const emails = new alien.Queue("emails").build()

const web = new alien.Container("web")
  .code({ type: "source", src: ".", toolchain: { type: "docker" } })
  .cpu(1)
  .memory("1Gi")
  .publicEndpoint("web", 3000)
  .healthCheck({ path: "/health" } as alien.HealthCheck)
  .autoScale({ min: 2, desired: 2, max: 10 })
  .environment({ PORT: "3000", NODE_ENV: "production" })
  .link(sessions)
  .link(emails)
  .permissions("web")
  .build()

const mailer = new alien.Worker("mailer")
  .code({ type: "source", src: "crates/mailer", toolchain: { type: "rust", binaryName: "mailer" } })
  .trigger({ type: "queue", queue: emails.ref() })
  .link(emails)
  .permissions("mailer")
  .build()

export default new alien.Stack("acme-app")
  .platforms(["aws", "gcp", "azure"])
  .add(sessions, "frozen")
  .add(emails, "frozen")
  .add(web, "live")
  .add(mailer, "live")
  .permissions({
    profiles: {
      web: { sessions: ["kv/data-read", "kv/data-write"], emails: ["queue/data-write"] },
      mailer: { emails: ["queue/data-read"] },
    },
  })
  .build()
