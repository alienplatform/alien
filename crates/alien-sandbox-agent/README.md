# Image startup

New GCP images assembled by Alien save the original OCI ENTRYPOINT, CMD, USER,
environment and working directory in root-owned metadata. The agent runs as
root with uid-split isolation and starts that command before serving session
operations. Root and absent USER get Docker's default capabilities without
NET_RAW; non-root image processes get none. Exec and jobs retain their non-root
identity, zero capabilities and no_new_privs. Image exits appear in health and
never trigger a restart.

The internal ALIEN_SANDBOX_IMAGE_COMMAND=true package marker enables this
contract. A package carrying it must have saved USER metadata and a root agent;
invalid configuration fails startup. Released packages without the marker retain
their previous startup and identity semantics. AWS packaging has not yet enabled
this marker; its released modes are described below.

# Azure native-file transport

Azure packages use `ALIEN_SANDBOX_AUTHORIZATION=native-file`. Before boot code
starts, the agent replaces `/opt/alien/native-token` with a random per-boot
credential, owned by root with mode 0600. The binding reads it through Azure's
authenticated native file API and sends bearer-authenticated requests to the
agent's published HTTPS port. It never runs a native shell command: Azure's
native exec can grant capabilities beyond those available to the image process.
The token grants access only to this sandbox's agent, never to cloud APIs.

The agent initially reports `awaiting-network-verification` and refuses session
operations. After the binding verifies the provider's policy, it sends an
authenticated `POST /v1/activate`. Activation starts the saved command once;
repeated activation never restarts it. A resumed process keeps that state, and a
fresh boot needs activation again after the provider policy is checked.

Exec, jobs and files use the shared agent protocol. Missing or invalid tokens
are refused before session operations. A fresh boot replaces the token; a
resume preserving the process keeps it. Create-time user environment and gateway
proxy settings remain defaults for boot and session commands; agent settings
are reserved. Released bindings without `agent: true` retain native operations.
Source-built Azure releases select this transport automatically. The controller switches
the image and transport together after Azure reports the replacement disk Ready.
Prebuilt-image inspection and migration of already-running native sessions remain
in progress.

# Released supervisor declarations

Released stacks may contain `privilegedSupervisor` with a `commandUid`. The
serialized field remains supported so those stacks can deploy, upgrade and delete
without changing identity or egress behavior. New Rust and TypeScript builders
no longer expose it. New image startup uses the saved OCI USER automatically.
The legacy enforcement behavior below applies only to those saved declarations.

Alien's agent is the privileged entrypoint. Before it serves requests or starts
image code, it installs a default-deny IPv4 nftables policy for the declared command uid,
prepares the declared
command identity, and reduces its own capabilities. The image's OCI Entrypoint
and Cmd then run as that identity. Exec, detached jobs, and services reached by
preview also run as that identity. A caller cannot choose the uid or change the
startup policy through an exec environment.

Commands have zero effective, permitted, inheritable and ambient capabilities,
an empty capability bounding set, and `no_new_privs`. They cannot regain privilege
through a setuid executable or file capability. An inherited seccomp filter denies
IPv6 sockets, io_uring socket creation, and alternate syscall ABIs. AWS MicroVMs
have an IPv6 default route, but their IPv6 netfilter tables are unavailable;
blocking IPv6 per command preserves the agent's AWS transport.

AWS cloud egress remains open. Native nftables uid matching restricts command
traffic while preserving the root traffic AWS needs to initialize its internal
transport after image restore. The iptables owner extension is unavailable on
the guest kernel. This mode creates no egress connector, proxy, or
firewall resources. AWS's extra capability grant is an implementation detail;
the declaration exposes only the opt-in supervisor setting and the command uid.
AWS accepts only `ALL` in `AdditionalOsCapabilities`; named capabilities such
as `NET_ADMIN` are rejected by the image API. Omitting the grant leaves out
`NET_ADMIN`, so the agent cannot install its firewall. See the
[AWS capability contract](https://docs.aws.amazon.com/lambda/latest/dg/microvms-images.html#microvms-images-os-capabilities).

Before creating worker threads, the supervisor reduces its effective, permitted,
and bounding sets to Docker's default capabilities without `NET_RAW`, plus
`NET_ADMIN` when it installs guest network rules. The image spawn primitive
always removes `NET_ADMIN`; it retains the common root set so a supervisor can
start workers under other uids. Backends without guest network rules need only
the common root set. It clears ambient and inheritable capabilities and sets
`no_new_privs`. Missing required capabilities fail startup. Seccomp installation after
`no_new_privs` needs no extra capability. The legacy `allowDomains` mode also uses
`SYS_ADMIN` during bootstrap to replace AWS's read-only `/etc/hosts` mount; that
capability is removed before worker threads or guest code start. Every image
command, exec, and detached job receives zero capabilities in all five sets.

`deny` grants commands no routed egress or DNS. `allowDomains` resolves the declared
names before starting commands, pins public IPv4 addresses in `/etc/hosts`, and
permits traffic only to those addresses. Commands cannot send DNS queries, including
to a loopback resolver. AWS snapshots these addresses into the image version. Rebuild and redeploy the
image to refresh them; restarting or resuming a session retains the pinned addresses. This is an IP allowlist: other hostnames or services sharing an
allowed IP are reachable. Wildcards and IPv6-only destinations are unsupported.
`allow` permits public IPv4 egress and the configured DNS resolvers, while denying
private/link-local destinations. Loopback services and replies to inbound agent or
preview connections remain available in every policy.

Remote raw cloud grants are refused for this mode: their holders could select a
retained image version with a different uid or policy. Use an ordinary workload
binding, which selects the active version.

Local, Kubernetes, Azure, and GCP currently refuse this opt-in at plan time. Local
uses Docker exec and Kubernetes runs a capability-free agent; neither can honor
this supervisor contract today. Their ordinary sandbox behavior continues to work.

## Build a compatible bundle

The agent programs nftables through native netlink; the image needs no `nft` or
`iptables` executable. The kernel must support IPv4 nftables and the bootstrap
process needs `NET_ADMIN`. Older bundles cannot enable this setting: startup fails
unless they preserve the base image's OCI command as root-owned metadata.

Managed gateway images on AWS set `ALIEN_SANDBOX_EGRESS_RUN_HOOK=true`. This
defers firewall installation until the authenticated Run hook, allowing the
platform to finish restoring its snapshot first. Until installation succeeds,
health reports `awaiting-run-hook`, token installation and session operations
are refused, and the image command remains stopped. The hook installs the same
rules with native netlink. A cgroup socket-creation program tags the agent and
its descendants independently of UID. Trusted agent sockets override that tag;
provider restore processes remain outside the cgroup. The guest cgroup mount is
read-only before guest code runs, and guest capabilities cannot change socket
marks or remount it writable. An inherited seccomp filter denies BPF, including
program detach through a writable subtree mounted in a new user namespace.
Provider mounts stay writable so suspend can freeze the workload. This preserves provider DNS,
metadata and ingress across snapshot restore while filtering guest root and every
UID it creates. Missing cgroup/BPF support prevents startup. The external network boundary remains
required even if guest root is compromised. Internet mode then waits
for its token and network self-check before starting guest code. Other backends
install the firewall at process startup by default. Legacy object policies keep
their existing startup behavior.

The authenticated Resume hook clears readiness and repeats the path checks before
accepting session operations. Checks include supervisor TCP to `1.1.1.1:443` and
UDP DNS to `8.8.8.8:53`, plus a child with the command's exact UID/GID and zero
capabilities. The child checks metadata HTTP at `169.254.169.254:80`, public HTTP,
and unmarked UDP DNS; only the synthetic local DNS answer is accepted. Supervisor
metadata stays available to the platform's restore path. A failed check leaves
session routes closed; a later successful Resume can recover. Resume verifies
the installed identity against the gateway's current policy, so renewal may arrive
before or after the hook. A rejected identity waits for renewal. Session requests
wait up to 15 seconds for lifecycle checks, and early identity delivery waits for
Run to install routing. Health reports `verifying-egress-path` during checks.

Gateway-mode command listeners cannot reply to external inbound connections.
Otherwise an incoming socket would provide an uninspected outbound channel.
Reach previews through the agent's loopback path. System/control listeners retain
their own return traffic. Released legacy object policies retain their existing
inbound-reply behavior.

Pull the intended ARM64 base image, then package it with the inspected command:

```sh
docker pull --platform linux/arm64 example/sandbox:1
cargo run -p alien-build --example sandbox-bundle -- \
  --privileged-supervisor --agent-binary ./alien-sandbox-agent-arm64 \
  example/sandbox:1 ./bundle.zip
```

The binary must be built for ARM64 Linux. `--agent-image <reference>` can replace
`--agent-binary <path>`. The bundle copies the inspected image command, environment,
and working directory to `/opt/alien/image-command.json`, owned by root and outside
the command-writable session directory. It never starts the base image command as
root. Upload the bundle to the S3 location declared in `code.image`.

## Validation

`cargo test -p alien-sandbox-agent` exercises the irreversible uid/capability drop,
IPv4 socket availability, denied IPv6/io_uring operations, and execution of saved
image commands with their image environment. `cargo test -p alien-core --lib
resources::sandbox` checks plan-time backend refusals and declaration-owned identity.
`cargo test -p alien-sandbox-agent --test privileged_supervisor -- --ignored`
runs the IPv4 allowlist, DNS denial (including loopback), rule-mutation refusal,
image identity, and command cancellation checks in a private Docker network
namespace, including replies from an unprivileged preview job to a host client
across the Docker bridge. It requires local Linux Docker with NET_ADMIN and internet access.

Live qualification must additionally run this agent on a MicroVM, verify allowed
IPv4 traffic and denied traffic to another address, attempt to change OUTPUT rules
through exec, and verify the image command's uid and capabilities. Delete all probe
resources afterward.

The ignored agent unit test
`gateway_egress::guest_network_tests::root_and_child_uids_keep_redirects_and_cannot_escape`
runs the native cgroup and nftables code in a disposable root MicroVM. It proves
TCP and DNS redirection for root, uid 2000 and uid 60000, and refusal of mark changes
and cgroup escape. It changes the VM's firewall and cgroup mounts; never run it on
a shared host. Set `ALIEN_ROUTING_RESUME_BARRIER=/tmp/routing-go` to run two rounds:
after the first, the test writes `/tmp/routing-go.ready`; suspend and resume the
MicroVM, then create `/tmp/routing-go` to release the second round. Both rounds
must pass within three minutes. Delete the disposable VM afterward. This test
qualifies routing, not the external gateway policy or full boot contract.

Health includes an `imageCommand` object with `status` (`notConfigured`, `pending`,
`empty`, `running`, `exited`, or `failed`). An exit records `code` or `signal` and
is logged without restarting the image process. Once network checks pass, spawn
must complete before a session operation proceeds, but the agent does not wait
for application readiness. Released images that never configured command startup
retain `notConfigured` until rebuilt with startup support.
