# Privileged supervisor mode

An AWS sandbox may opt into agent-owned egress enforcement:

```typescript
new Sandbox("analysis")
  .code({ type: "image", image: "s3://example-bundles/analysis/bundle.zip" })
  .privilegedSupervisor({ commandUid: 60001 })
  .egress({ mode: "allowDomains", domains: ["example.com"] })
  .lifecycle({})
```

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
and bounding sets to `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `KILL`, `SETGID`, `SETUID`,
`SETPCAP`, and `NET_ADMIN`. It clears ambient and inheritable capabilities and sets
`no_new_privs`. These allow file projection and transfer, command cancellation,
irreversible identity drops, and firewall installation. Seccomp installation after
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
rules with native netlink, scoped to the command UID. All image commands, execs,
and jobs irreversibly drop to that identity before running. Other system identities
retain their DNS, metadata and ingress traffic across snapshot restore; there is no
hardcoded platform UID exemption. A command UID that collides with a platform UID
is still filtered, never silently exempted. The external network boundary remains
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
