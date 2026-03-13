import * as alien from "@aliendotdev/core"

const fn = new alien.Function("my-function")
  .code({
    type: "source",
    toolchain: { type: "typescript" },
    src: ".",
  })
  .permissions("execution")
  .build()

const stack = new alien.Stack("my-stack")
  .add(fn, "frozen")
  .permissions({
    profiles: {
      execution: {
        "*": ["function/execute"]
      }
    }
  })
  .build()

export default stack
