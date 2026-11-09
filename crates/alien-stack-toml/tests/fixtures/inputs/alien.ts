import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  runner_region: alien.string({
    providedBy: "developer",
    required: true,
    label: "Runner region",
    description: "Region key this deployment's workers consume.",
    minLength: 1,
    pattern: "^[a-z0-9-]+$",
    env: { name: "RUNNER_REGION", targetResources: ["worker"] },
  }),
  store_backend: alien.enum(["sqlite", "cas"], {
    providedBy: "developer",
    required: true,
    label: "Store backend",
    description: "Backend that stores registry state.",
    default: "sqlite",
    env: "STORE_BACKEND",
  }),
  cache_size_gb: alien.integer({
    providedBy: "developer",
    required: true,
    label: "Cache size",
    description: "Largest local cache, in GiB.",
    default: 100,
    min: 1,
    max: 4096,
    env: ["CACHE_SIZE_GB", { name: "LEGACY_CACHE_GB", targetResources: ["work*"] }],
  }),
  sample_rate: alien.number({
    providedBy: "developer",
    required: false,
    label: "Trace sample rate",
    description: "",
    min: 0,
    max: 1.0,
    default: 0.25,
  }),
  dry_run: alien.boolean({
    providedBy: ["developer", "deployer"],
    required: true,
    label: "Dry run",
    description: "Observe decisions without applying them.",
    default: false,
    env: { name: "DRY_RUN", targetResources: ["worker"] },
  }),
  prewarm_images: alien.stringList({
    providedBy: "developer",
    required: false,
    label: "Prewarm images",
    description: "",
    default: ["ghcr.io/acme/runner:latest"],
    maxItems: 8,
    platforms: ["machines"],
    env: "PREWARM_IMAGES",
  }),
  activation_token: alien.secret({
    providedBy: "deployer",
    required: true,
    label: "Activation token",
    description: "Short-lived token used to enroll this runtime.",
    minLength: 32,
    env: "ACTIVATION_TOKEN",
  }),
  managed_bucket: alien.boolean({
    providedBy: "deployer",
    required: true,
    platforms: ["aws"],
    default: true,
    label: "Create a data bucket",
    description: "Create an Alien-managed bucket for this deployment's data.",
  }),
  api_token: alien.secret({
    providedBy: "developer",
    required: true,
    label: "API token",
    description: "Token the worker uses to call the API.",
    env: { name: "API_TOKEN", targetResources: ["worker"] },
  }),
})

const data = new alien.Storage("data").enabled(inputs.managed_bucket).build()

const worker = new alien.Daemon("worker")
  .code({ type: "image", image: "ghcr.io/acme/worker:2.0.0" })
  .build()

export default new alien.Stack("inputs-demo")
  .inputs(inputs)
  .platforms(["machines", "aws"])
  .add(data, "frozen")
  .add(worker, "live")
  .build()
