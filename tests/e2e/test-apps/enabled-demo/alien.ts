import * as alien from "@alienplatform/core"

// A deployer-input gate per resource type, in matched on/off pairs. The e2e
// answers the four `*On` inputs true and the four `*Off` inputs false at apply
// time, then verifies each on-resource (and its grant) exists in the cloud
// while each off-resource is absent. Defaults are false so the on-resources
// only appear when the harness actually threads the answer through — otherwise
// the test would pass without proving the gate value was applied.
const io = alien.inputs({
  kvOn: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the on key-value store",
    description: "Answered true by the e2e; the store must exist.",
  }),
  kvOff: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the off key-value store",
    description: "Answered false by the e2e; the store must be absent.",
  }),
  storageOn: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the on object store",
    description: "Answered true by the e2e; the bucket must exist.",
  }),
  storageOff: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the off object store",
    description: "Answered false by the e2e; the bucket must be absent.",
  }),
  queueOn: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the on queue",
    description: "Answered true by the e2e; the queue must exist.",
  }),
  queueOff: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the off queue",
    description: "Answered false by the e2e; the queue must be absent.",
  }),
  vaultOn: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the on secret store",
    description: "Answered true by the e2e; its grant must exist.",
  }),
  vaultOff: alien.boolean({
    providedBy: "deployer",
    required: false,
    default: false,
    label: "Enable the off secret store",
    description: "Answered false by the e2e; its grant must be absent.",
  }),
})

// Ungated positive control: proves setup actually provisioned resources, so an
// absent off-resource is a real gate outcome rather than an empty deployment.
// Frozen (setup-created) so the worker's read grant bakes into the execution
// role at setup. A Live data resource would defer that grant to a runtime
// PutRolePolicy on the setup-owned role, which the management role is not
// permitted to do — orthogonal to the gate this app exists to exercise.
const state = new alien.Kv("state").build()

const kvOn = new alien.Kv("optional-kv-on").enabled(io.kvOn).build()
const kvOff = new alien.Kv("optional-kv-off").enabled(io.kvOff).build()
const storageOn = new alien.Storage("optional-storage-on").enabled(io.storageOn).build()
const storageOff = new alien.Storage("optional-storage-off").enabled(io.storageOff).build()
const queueOn = new alien.Queue("optional-queue-on").enabled(io.queueOn).build()
const queueOff = new alien.Queue("optional-queue-off").enabled(io.queueOff).build()
const vaultOn = new alien.Vault("optional-vault-on").enabled(io.vaultOn).build()
const vaultOff = new alien.Vault("optional-vault-off").enabled(io.vaultOff).build()

const agent = new alien.Worker("agent")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .publicEndpoint("api")
  .permissions("execution")
  .build()

export default new alien.Stack("enabled-demo")
  .inputs(io)
  .add(state, "frozen")
  .add(kvOn, "frozen")
  .add(kvOff, "frozen")
  .add(storageOn, "frozen")
  .add(storageOff, "frozen")
  .add(queueOn, "frozen")
  .add(queueOff, "frozen")
  .add(vaultOn, "frozen")
  .add(vaultOff, "frozen")
  .add(agent, "live")
  .permissions({
    profiles: {
      // Each gated resource carries its own resource-scoped grant so the e2e can
      // assert the grant follows the gate (present when on, gone when off). The
      // worker binds only the ungated `state` store; it depends on no gated
      // resource, so the ungated-dependent-of-a-gated-resource preflight stays
      // satisfied.
      execution: {
        state: ["kv/data-read"],
        "optional-kv-on": ["kv/data-read"],
        "optional-kv-off": ["kv/data-read"],
        "optional-storage-on": ["storage/data-read"],
        "optional-storage-off": ["storage/data-read"],
        "optional-queue-on": ["queue/data-read"],
        "optional-queue-off": ["queue/data-read"],
        "optional-vault-on": ["vault/data-read"],
        "optional-vault-off": ["vault/data-read"],
      },
    },
  })
  .build()
