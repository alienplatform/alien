import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  runner_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create code runner",
    description: "Whether this deployment gets the code runner.",
    default: true,
  }),
})

const runner = new alien.Sandbox("runner")
  .displayName("Code runner")
  .description("Runs customer code in isolation.")
  .enabled(inputs.runner_enabled)
  .code({ type: "image", image: "s3://acme-bundles/runner/{region}/rootfs.tar" })
  .privateBaseImage("123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/base:1.0")
  .limits({ cpu: "2", memory: "4Gi", disk: "8Gi", maxProcesses: 256 })
  .egress("internet")
  .defaultPolicy({ allow: ["api.acme.example", "*.cdn.acme.example:443"] })
  .lifecycle({ maxLifetimeSeconds: 7_200, idlePauseSeconds: 600 })
  .previewPorts([3000, 8080])
  .build()

// The builder has no method for the privileged supervisor, so the generated config field is
// set directly.
;(runner.config as { privilegedSupervisor?: { commandUid: number } }).privilegedSupervisor = {
  commandUid: 1000,
}

const builder = new alien.Sandbox("builder")
  .code({
    type: "source",
    src: "crates/builder",
    toolchain: { type: "rust", binaryName: "builder" },
  })
  .egress("none")
  .defaultPolicy("internet")
  .lifecycle({ maxLifetimeSeconds: 3_600 })
  .build()

const mirror = new alien.Sandbox("mirror")
  .code({ type: "image", image: "s3://acme-bundles/mirror/{region}/rootfs.tar" })
  .egress({ mode: "allowDomains", domains: ["mirror.acme.example"] })
  .lifecycle({})
  .build()

export default new alien.Stack("acme-sandbox")
  .inputs(inputs)
  .platforms(["aws"])
  .add(runner, "live", { remoteAccess: true })
  .add(builder, "frozen")
  .add(mirror, "frozen")
  .build()
