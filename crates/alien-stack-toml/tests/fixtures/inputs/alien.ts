import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  runnerRegion: alien.string({
    providedBy: "developer",
    required: true,
    label: "Runner region",
    description: "Region key this deployment's workers consume.",
    minLength: 1,
    pattern: "^[a-z0-9-]+$",
    env: { name: "RUNNER_REGION", targetResources: ["worker"] },
  }),
  storeBackend: alien.enum(["sqlite", "cas"], {
    providedBy: "developer",
    required: true,
    label: "Store backend",
    description: "Backend that stores registry state.",
    default: "sqlite",
    env: "STORE_BACKEND",
  }),
  cacheSizeGb: alien.integer({
    providedBy: "developer",
    required: true,
    label: "Cache size",
    description: "Largest local cache, in GiB.",
    default: 100,
    min: 1,
    max: 4096,
    env: ["CACHE_SIZE_GB", { name: "LEGACY_CACHE_GB", targetResources: ["work*"] }],
  }),
  sampleRate: alien.number({
    providedBy: "developer",
    required: false,
    label: "Trace sample rate",
    description: "",
    min: 0,
    max: 1.0,
    default: 0.25,
  }),
  dryRun: alien.boolean({
    providedBy: ["developer", "deployer"],
    required: true,
    label: "Dry run",
    description: "Observe decisions without applying them.",
    default: false,
    env: { name: "DRY_RUN", targetResources: ["worker"] },
  }),
  prewarmImages: alien.stringList({
    providedBy: "developer",
    required: false,
    label: "Prewarm images",
    description: "",
    default: ["ghcr.io/acme/runner:latest"],
    maxItems: 8,
    platforms: ["machines"],
    env: "PREWARM_IMAGES",
  }),
  activationToken: alien.secret({
    providedBy: "deployer",
    required: true,
    label: "Activation token",
    description: "Short-lived token used to enroll this runtime.",
    minLength: 32,
    env: "ACTIVATION_TOKEN",
  }),
  managedBucket: alien.boolean({
    providedBy: "deployer",
    required: true,
    platforms: ["aws"],
    default: true,
    label: "Create a data bucket",
    description: "Create an Alien-managed bucket for this deployment's data.",
  }),
  apiToken: alien.secret({
    providedBy: "developer",
    required: true,
    label: "API_TOKEN",
    description: "Sets the API_TOKEN environment variable.",
    env: { name: "API_TOKEN", targetResources: ["worker"] },
  }),
})

const data = new alien.Storage("data").enabled(inputs.managedBucket).build()

const worker = new alien.Daemon("worker").code({ type: "image", image: "ghcr.io/acme/worker:2.0.0" }).build()

export default new alien.Stack("inputs-demo")
  .inputs(inputs)
  .platforms(["machines", "aws"])
  .add(data, "frozen")
  .add(worker, "live")
  .build()
