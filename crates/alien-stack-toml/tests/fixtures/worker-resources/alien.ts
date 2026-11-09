import * as alien from "@alienplatform/core"

const sized = new alien.Worker("sized")
  .code({ type: "image", image: "ghcr.io/acme/sized:1.0.0" })
  .memoryMb(1536)
  .timeoutSeconds(90)
  .concurrencyLimit(5)
  .permissions("sized")
  .build()

const megabytes = new alien.Worker("megabytes")
  .code({ type: "image", image: "ghcr.io/acme/megabytes:1.0.0" })
  .memoryMb(512)
  .timeoutSeconds(45)
  .permissions("megabytes")
  .build()

const defaults = new alien.Worker("defaults")
  .code({ type: "image", image: "ghcr.io/acme/defaults:1.0.0" })
  .permissions("defaults")
  .build()

const hour = new alien.Worker("hour")
  .code({ type: "image", image: "ghcr.io/acme/hour:1.0.0" })
  .timeoutSeconds(3600)
  .readinessProbe({ path: "/up" })
  .permissions("hour")
  .build()

const probed = new alien.Worker("probed")
  .code({ type: "image", image: "ghcr.io/acme/probed:1.0.0" })
  .publicEndpoint("main")
  .readinessProbe({ method: "HEAD" })
  .permissions("probed")
  .build()

export default new alien.Stack("acme-worker-resources")
  .platforms(["aws", "gcp"])
  .add(sized, "live")
  .add(megabytes, "live")
  .add(defaults, "live")
  .add(hour, "live")
  .add(probed, "live")
  .permissions({
    profiles: { sized: {}, megabytes: {}, defaults: {}, hour: {}, probed: {} },
  })
  .build()
