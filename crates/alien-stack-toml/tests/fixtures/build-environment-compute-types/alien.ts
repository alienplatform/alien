import * as alien from "@alienplatform/core"

const cache = new alien.Storage("cache").build()

const defaultSize = new alien.Build("default-size").permissions("default-size").build()

const small = new alien.Build("small").computeType("small").permissions("small").build()

const medium = new alien.Build("medium").computeType("medium").permissions("shared").build()

const large = new alien.Build("large")
  .computeType("large")
  .permissions("shared")
  .link(cache)
  .build()

const extraLarge = new alien.Build("extra-large")
  .computeType("x-large")
  .environment({ TOOLCHAIN: "stable", CI: "true" })
  .permissions("extra-large")
  .build()

export default new alien.Stack("acme-build-sizes")
  .platforms(["aws"])
  .add(cache, "frozen")
  .add(defaultSize, "live")
  .add(small, "live")
  .add(medium, "live")
  .add(large, "live")
  .add(extraLarge, "live")
  .permissions({
    profiles: {
      shared: { cache: ["storage/data-read"] },
      "default-size": {},
      small: {},
      "extra-large": {},
    },
  })
  .build()
