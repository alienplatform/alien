import * as alien from "@alienplatform/core"

const files = new alien.Storage("files").build()

const worker = new alien.Function("worker")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .ingress("private")
  .link(files)
  .permissions("execution")
  .build()

export default new alien.Stack("remote-worker")
  .add(files, "frozen")
  .add(worker, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["storage/data-read", "storage/data-write"],
      },
    },
  })
  .build()
