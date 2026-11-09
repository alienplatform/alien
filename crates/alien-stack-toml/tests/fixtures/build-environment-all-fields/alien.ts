import * as alien from "@alienplatform/core"

const artifacts = new alien.Storage("artifacts").build()

const release = new alien.Build("release")
  .computeType("large")
  .link(artifacts)
  .permissions("release")
  .environment({ CI: "true", PARALLELISM: "8", CHANNEL: "stable" })
  .build()

export default new alien.Stack("acme-builds")
  .platforms(["aws", "gcp", "azure"])
  .add(artifacts, "frozen")
  .add(release, "live")
  .permissions({
    profiles: { release: { artifacts: ["storage/data-read", "storage/data-write"] } },
  })
  .build()
