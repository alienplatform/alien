import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  uploads_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    default: true,
    label: "Accept uploads",
    description: "Create the uploads bucket and the daemon that fills it.",
  }),
  archive_enabled: alien.boolean({
    providedBy: ["developer", "deployer"],
    required: false,
    default: false,
    label: "Keep an archive",
    description: "",
    platforms: ["aws", "gcp"],
  }),
  sessions_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    default: true,
    label: "Store sessions",
    description: "Create the session store.",
  }),
  jobs_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    default: false,
    label: "Run background jobs",
    description: "Create the job queue and the worker that drains it.",
    platforms: ["aws", "gcp"],
  }),
  customer_keys_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    default: false,
    label: "Customer-managed keys",
    description: "Create a vault for keys the deployer manages.",
  }),
  renderer_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    default: true,
    label: "Render thumbnails",
    description: "Run the thumbnail renderer on the render pool.",
    platforms: ["machines"],
  }),
})

const compute = new alien.ComputeCluster("compute")
  .pool("render", {
    requirements: { cpu: 4, memory: "8Gi" },
    scale: { type: "fixed", machines: 2 },
  })
  .build()

const uploads = new alien.Storage("uploads")
  .versioning(true)
  .enabled(inputs.uploads_enabled)
  .build()

const uploader = new alien.Daemon("uploader")
  .code({ type: "image", image: "ghcr.io/acme/uploader:1.8.0" })
  .cluster("compute")
  .link(uploads)
  .permissions("uploader")
  .enabled(inputs.uploads_enabled)
  .build()

const archive = new alien.Storage("archive").enabled(inputs.archive_enabled).build()

const sessions = new alien.Kv("sessions").enabled(inputs.sessions_enabled).build()

const jobs = new alien.Queue("jobs").enabled(inputs.jobs_enabled).build()

const ingest = new alien.Worker("ingest")
  .code({ type: "image", image: "ghcr.io/acme/ingest:1.8.0" })
  .link(jobs)
  .permissions("ingest")
  .enabled(inputs.jobs_enabled)
  .build()

const customerKeys = new alien.Vault("customer-keys").enabled(inputs.customer_keys_enabled).build()

const renderer = new alien.Container("renderer")
  .code({ type: "image", image: "ghcr.io/acme/renderer:2.1.0" })
  .cluster("compute")
  .pool("render")
  .cpu(1)
  .memory("1Gi")
  .port(8080)
  .permissions("renderer")
  .enabled(inputs.renderer_enabled)
  .build()

export default new alien.Stack("acme-media")
  .inputs(inputs)
  .platforms(["machines", "aws", "gcp"])
  .add(compute, "frozen")
  .add(uploads, "frozen")
  .add(uploader, "live")
  .add(archive, "live")
  .add(sessions, "frozen")
  .add(jobs, "frozen")
  .add(ingest, "live")
  .add(customerKeys, "frozen")
  .add(renderer, "live")
  .permissions({
    profiles: {
      uploader: { uploads: ["storage/data-read", "storage/data-write"] },
      ingest: {},
      renderer: {},
    },
  })
  .build()
