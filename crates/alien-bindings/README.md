# alien-bindings

Platform-agnostic binding abstractions for Alien applications. Defines traits for storage, KV, vault, queue, build, container, function, artifact registry, and service accounts, with per-platform provider implementations.

## Architecture

- `traits.rs` — Binding trait definitions (`Storage`, `Kv`, `Vault`, `Queue`, `Build`, etc.)
- `provider.rs` — `BindingsProvider`: holds all binding instances for a deployment, keyed by name
- `bindings.rs` — `Bindings`: app-facing entry point over `BindingsProvider` (`storage`, `kv`, `queue`, `vault`)
- `providers/` — Platform-specific implementations (AWS, GCP, Azure, Local, etc.)

## Feature Flags

- `openapi` — OpenAPI schema generation
- Platform features: `aws`, `gcp`, `azure`, `kubernetes`, `local`, `test`

## Usage

```rust
use alien_bindings::Bindings;
use object_store::path::Path;

let bindings = Bindings::from_env()?;
let storage = bindings.storage("files").await?;
storage.put(&Path::from("hello.txt"), data.into()).await?;
```

With the `platform-sdk` feature, trusted backend code can open the hosted
remote Storage surface through the deployment's assigned manager:

```rust,no_run
use alien_bindings::RemoteBindings;

let bindings = RemoteBindings::for_deployment(
    "dep_...",
    &std::env::var("ALIEN_API_TOKEN")?,
    None,
)
.await?;
let storage = bindings.storage("uploads").await?;
```

Remote v0 supports only Running, Frozen S3, GCS, and Azure Blob Storage
resources with remote access enabled. The API token and returned provider
credentials are backend secrets. Remote access grants the deployment management
identity exact object read, write, list, delete, and multipart permissions on
the selected bucket or container; it does not create a separate identity per
resource.

This replaces the former unscoped `/v1/resolve-credentials` flow. The dedicated
`RemoteBindings` type exposes only Storage; its handles refresh credentials and
rediscover manager assignment without exposing a non-refreshing provider.

## Azure sandbox gateway recovery

Azure `internet` egress is hosted by two gateway VMs that the setup package creates
in the deployment's VNet (a gateway subnet whose NSG admits only the sandbox and
workload subnets, with the deployment's NAT for the fixed outbound IP). Sandbox
subnets have no NAT and no route out, so the gateway is the only way to the
internet. It needs an image with the Alien sandbox agent: a catalog image is
refused at plan time, and the agent proves the gateway path before the ENTRYPOINT
starts.

Accepted limitations of this version:

- **Frozen.** The gateway VMs come from setup, and a gateway image update rides a
  setup re-run. The binding checks the gateway's protocol version through the admin
  API and refuses with "re-run setup to update the egress gateway" when it cannot
  serve it. The admin protocol stays backward compatible.
  A setup re-run never takes both gateways down together. The image does not ride the
  VMs' custom data (which would replace both VMs at once); a run command on each VM
  pulls the image, restarts the gateway and waits until it is ready, meaning it has
  caught up its policies from the other gateway. The second VM's command waits for the
  first, and a VM that is serving is not restarted unless its peer is serving. Before any
  update, a repair command on each VM (one at a time) restarts a gateway that is installed
  but not serving while the other serves, and never restarts the only serving gateway. Run
  commands run only when they are created or their script changes, so a re-run with no other
  change repairs nothing: to repair an unhealthy VM, increase the module variable
  `<network>_sandbox_gateway_repair_generation` (a number, default 0) and apply. The gateway
  opens its proxy port only after it has caught up, so while a gateway restarts (or a new
  image never catches up) its port is closed and agents' connects fail over to the other
  gateway; a failed wait fails the apply and stops the roll with the other gateway serving.
  Idle sandboxes and new connections keep their egress through the update; a connection
  that is open through the gateway being restarted is closed and the client reconnects.
  Setups written by this module's earlier versions carried the image in custom data; they
  keep their VMs (`ignore_changes` on custom data) and only the gateway container restarts,
  one VM at a time.
  Only the image rolls one at a time. A change that replaces or resizes the VMs
  themselves (a size change, a new OS image version) acts on both VMs in the same apply,
  and the gateways lose their in-memory policy together until the application re-registers
  it: do those during a maintenance window.
- **DNS.** An allowlist limits which hosts the gateway connects to; it cannot stop data
  leaving through DNS queries, which the sandbox's resolver answers.
- **Patching.** The VMs use platform-orchestrated patching (`AutomaticByPlatform`),
  so Azure patches the OS and they sit in an availability set so a reboot takes one
  gateway down at a time. A running sandbox keeps its egress while one VM is down:
  the agent's proxy address is a loopback forwarder that dials each connection to
  the last good replica, then the other (`alien-sandbox-agent` `proxy_failover`). A CONNECT that a replica closes before any answer byte is replayed once on the other replica (up to 32 KiB of the request is kept; any other request is never replayed). The gateway closes without a byte at a connection limit, so a replay can let a sandbox reach up to twice its per-identity limit (the limit is per replica); accepted.
  A new sandbox's first grant is accepted once one replica holds it (bounded to 20 s;
  the replica that was away starts empty, refuses until it has synced from its peer, so
  a missing grant only ever denies). `update_egress` needs both replicas and is refused
  with a retryable error while one is down, because a replica that missed a narrowing would
  keep serving the wider policy; a delete reaches every replica that answers. The gateway
  container image is pinned by setup and is updated only by a setup re-run.
- **Encryption at host.** Encryption at host is off on the gateway VMs and is not configurable
  in this module yet: it needs the `Microsoft.Compute/EncryptionAtHost` feature registered on the
  subscription (`az feature register --namespace Microsoft.Compute --name EncryptionAtHost`), and
  the apply fails without it. Ask us if you need it on. The OS disks are still encrypted at rest
  with platform-managed keys, and the VMs hold no customer data (policies live in memory).
- **Gateway identity.** Each VM's managed identity reads only two Key Vault secrets
  (`sandbox-egress-gateway-config`, `sandbox-egress-replica-secret`), never the
  signing key. The container reaches the identity endpoint through the docker bridge
  to fetch them.

The gateway keeps its policy in memory. After all gateway replicas lose their state,
proxy requests receive HTTP 403 with reason `no policy` until the application
reapplies its current policy with `updateEgress`, or `getOrCreate` with an explicit
`egress` argument.

For an existing sandbox, `getOrCreate` without `egress` must preserve the stored
policy, including its absence after total state loss. Defaults apply only when
creating a sandbox. Reconnecting never restores an older create-time policy
after the application has narrowed access.

Azure's create-time proxy token proves identity only. Issuing it before provider
creation does not register a policy or grant access. The binding publishes the
initial policy only after verifying the provider's network, native policy and
identity label. Applications must retain the policy they intend to reapply;
there is no automatic recovery from the token or sandbox environment.

Gateway identities expire after 24 hours and are never renewed on reconnect.
The deployment controller reaps sessions in Internet-mode groups at that ceiling,
using creation timestamps and the earlier identity deadline label. Cleanup runs
on reconciliation; a stopped controller, terminal failure, or unavailable Azure
API delays deletion, but does not extend the signed identity. Existing native
modes retain their lifetime behavior, and per-create timeout requests remain
unsupported.

The native transport uses Partial inspection with one catch-all `*` deny, together with
setup-owned subnet rules. The deny closes the :80/:443 interposer (a connect there succeeds,
and the platform answers an empty 403 with no origin data); the subnet NSG allows only the
gateway's proxy port and drops every other public port, other private address and the
gateway admin port. Each layer was proven alone on a live setup: with the NSG's Internet
deny removed, :80/:443 stay contained and other ports leak; with the policy removed the
NSG drops everything. Azure refuses a `host:port` rule pattern, and an IP host rule would
not open a TCP port, so the policy carries no gateway rule. The packaged agent's startup
gate proves the gateway connects and that public canaries on ports the interposer does not
answer (8443 and 8080) drop, retrying for 60 s, before the ENTRYPOINT starts.
The binding requires the `default` VNet connection and checks the effective
policy and identity label before granting access. A trusted holder of Azure's
raw Data Owner credential can create a sandbox outside this binding and omit
that connection; this is not enforcement against that management caller.

## Adding New Providers

1. Create a new module under `src/providers/` implementing `BindingsProvider`
2. Add feature flag in `Cargo.toml`
3. Update `get_platform_provider()` in `lib.rs`

## Adding New Binding Types

1. Define the trait in `src/traits.rs`
2. Add method to `BindingsProvider`
3. Implement for each provider
