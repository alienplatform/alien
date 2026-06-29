import * as alien from "@alienplatform/core"

const app = new alien.Container("app")
  .code({ type: "source", src: ".", toolchain: { type: "docker", dockerfile: "Dockerfile" } })
  .cpu(0.5)
  .memory("512Mi")
  .port(3000)
  .publicEndpoint("web", 3000, "http")
  // Next's standalone server reads these; HOSTNAME=0.0.0.0 binds all interfaces.
  .environment({ PORT: "3000", HOSTNAME: "0.0.0.0" })
  .permissions("app")
  .build()

export default new alien.Stack("nextjs-app")
  .platforms(["aws", "gcp", "azure"])
  .add(app, "live")
  .permissions({ profiles: { app: {} } }) // no linked resources → empty profile
  .build()
