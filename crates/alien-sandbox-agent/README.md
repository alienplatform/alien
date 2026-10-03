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
The supervisor retains network administration and the capabilities needed to
prepare command-owned files, drop identities, and cancel commands.

`deny` grants commands no routed egress or DNS. `allowDomains` resolves the declared
names before starting commands, pins public IPv4 addresses in `/etc/hosts`, and
permits traffic only to those addresses. Commands cannot send DNS queries, including
to a loopback resolver. The addresses stay pinned for the session lifetime; restart
to refresh them. This is an IP allowlist: other hostnames or services sharing an
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

The image must include `/usr/sbin/nft`. Alien's default sandbox
base includes nftables. Older bundles cannot enable this setting: startup fails
unless they preserve the base image's OCI command as root-owned metadata.

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
