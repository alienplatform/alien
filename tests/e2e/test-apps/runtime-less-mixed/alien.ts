import * as alien from "@alienplatform/core"

const index = new alien.Kv("index").build()

const typescriptContainer = new alien.Container("typescript-container")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .cpu(0.25)
  .memory("256Mi")
  .commandsEnabled(true)
  .link(index)
  .permissions("execution")
  .build()

const rustDaemon = new alien.Daemon("rust-daemon")
  .code({
    type: "source",
    src: "./daemon",
    toolchain: { type: "rust", binaryName: "runtime-less-rust-daemon" },
  })
  .commandsEnabled(true)
  .link(index)
  .permissions("execution")
  .build()

export default new alien.Stack("runtime-less-mixed")
  .platforms(["local", "kubernetes"])
  .add(index, "frozen")
  .add(typescriptContainer, "live")
  .add(rustDaemon, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["kv/data-read", "kv/data-write"],
      },
    },
  })
  .build()
