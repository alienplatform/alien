import * as alien from "@alienplatform/core"

const app = new alien.Container("app")
  .code({
    type: "source",
    src: ".",
    toolchain: {
      type: "python",
      pythonVersion: "3.12",
      command: ["python", "app.py"],
    },
  })
  .cpu(0.25)
  .memory("256Mi")
  .port(8080)
  .publicEndpoint("web", 8080, "http")
  .permissions("app")
  .build()

export default new alien.Stack("python-container")
  .platforms(["aws", "gcp", "azure"])
  .add(app, "live")
  .permissions({ profiles: { app: {} } })
  .build()
