// Local pull E2E app: one Rust SOURCE Container with a pull command receiver
// and a direct in-process KV binding. The OSS local platform is the only
// target — cloud container coverage lives in the platform repo's E2E.
import * as alien from "@alienplatform/core"

const index = new alien.Kv("index").build()

const indexer = new alien.Container("indexer")
  .code({
    type: "source",
    src: "./",
    toolchain: { type: "rust", binaryName: "container-rust-indexer" },
  })
  .cpu(0.25)
  .memory("256Mi")
  .commandsEnabled(true)
  .link(index)
  .permissions("execution")
  .build()

export default new alien.Stack("container-rust")
  .platforms(["local"])
  .add(index, "frozen")
  .add(indexer, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["kv/data-read", "kv/data-write"],
      },
    },
  })
  .build()
