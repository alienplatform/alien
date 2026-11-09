import * as alien from "@alienplatform/core"

const data = new alien.Storage("data").build()

export default new alien.Stack("acme-dynamic-repositories")
  .platforms(["aws", "gcp"])
  .add(data, "frozen")
  .dynamicContainerRepositories([
    "ghcr.io/acme/runner",
    "registry.acme.example:5000/team/tools/builder",
    "docker.io/library/alpine",
    "us-docker.pkg.dev/acme-prod/images/sandbox-base",
  ])
  .build()
