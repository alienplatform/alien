import * as alien from "@alienplatform/core"

const uploads = new alien.Storage("uploads").build()
const jobs = new alien.Queue("jobs").build()
const sessions = new alien.Kv("sessions").build()

const writer = new alien.Container("writer")
  .code({ type: "image", image: "ghcr.io/acme/writer:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .link(uploads)
  .link(jobs)
  .permissions("writer")
  .build()

const viewer = new alien.Container("viewer")
  .code({ type: "image", image: "ghcr.io/acme/viewer:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .link(uploads)
  .link(sessions)
  .permissions("reader")
  .build()

const auditor = new alien.Container("auditor")
  .code({ type: "image", image: "ghcr.io/acme/auditor:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .permissions("reader")
  .build()

const plain = new alien.Container("plain")
  .code({ type: "image", image: "ghcr.io/acme/plain:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .permissions("plain")
  .build()

const sweeper = new alien.Container("sweeper")
  .code({ type: "image", image: "ghcr.io/acme/sweeper:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .permissions("everything")
  .build()

export default new alien.Stack("acme-access")
  .platforms(["aws", "gcp"])
  .add(uploads, "frozen")
  .add(jobs, "frozen")
  .add(sessions, "frozen")
  .add(writer, "live")
  .add(viewer, "live")
  .add(auditor, "live")
  .add(plain, "live")
  .add(sweeper, "live")
  .permissions({
    profiles: {
      reader: { uploads: ["storage/data-read"], sessions: ["kv/data-read"] },
      writer: {
        uploads: ["storage/data-read", "storage/data-write"],
        jobs: ["queue/data-write"],
      },
      plain: {},
      everything: { "*": ["storage/data-read", "kv/data-read"] },
    },
  })
  .build()
