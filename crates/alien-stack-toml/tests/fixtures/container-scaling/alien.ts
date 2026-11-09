import * as alien from "@alienplatform/core"

const elastic = new alien.Container("elastic")
  .code({ type: "image", image: "ghcr.io/acme/elastic:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .autoScale({ min: 2, desired: 2, max: 10 })
  .permissions("elastic")
  .build()

const warm = new alien.Container("warm")
  .code({ type: "image", image: "ghcr.io/acme/warm:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .autoScale({ min: 1, desired: 4, max: 8 })
  .permissions("warm")
  .build()

const targeted = new alien.Container("targeted")
  .code({ type: "image", image: "ghcr.io/acme/targeted:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .autoScale({
    min: 3,
    desired: 3,
    max: 30,
    targetCpuPercent: 55.5,
    targetHttpInFlightPerReplica: 50,
  })
  .permissions("targeted")
  .build()

const fixed = new alien.Container("fixed")
  .code({ type: "image", image: "ghcr.io/acme/fixed:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .replicas(3)
  .permissions("fixed")
  .build()

export default new alien.Stack("acme-scaling")
  .platforms(["aws"])
  .add(elastic, "live")
  .add(warm, "live")
  .add(targeted, "live")
  .add(fixed, "live")
  .permissions({ profiles: { elastic: {}, warm: {}, targeted: {}, fixed: {} } })
  .build()
