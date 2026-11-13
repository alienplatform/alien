import * as alien from "@alienplatform/core"

const single = new alien.Container("single")
  .code({ type: "image", image: "ghcr.io/acme/single:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .port(8080)
  .permissions("single")
  .build()

const multiple = new alien.Container("multiple")
  .code({ type: "image", image: "ghcr.io/acme/multiple:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .ports([9000, 9001, 9002])
  .permissions("multiple")
  .build()

export default new alien.Stack("acme-ports")
  .platforms(["aws"])
  .add(single, "live")
  .add(multiple, "live")
  .permissions({
    profiles: { single: {}, multiple: {} },
  })
  .build()
