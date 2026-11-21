# Sandbox links

`sandbox.link(service)` lets every sandbox of a Sandbox resource call a service of the same
deployment, such as the app's own API, without that service being public.

```ts
import * as alien from "@alienplatform/core"

const api = new alien.Container("api")
  .code({ type: "image", image: "ghcr.io/example/api:1.0" })
  .port(8080)
  .cpu(1)
  .memory("1Gi")
  .build()

const tools = new alien.Sandbox("tools")
  .code({ type: "image", image: "ghcr.io/example/tools:1.0" })
  .egress("internet")
  .lifecycle({ maxLifetimeSeconds: 3600 })
  .link(api)
  .build()
```

Rust declares the same with `Sandbox::new("tools".into())…link(&api)`.

## What a link does

- A sandbox reaches the linked service through the deployment's egress gateway, the same way
  it reaches the internet: with the sandbox's runtime policy, the gateway's DNS checks, its
  periodic re-checks and its connection log. Only the gateway's last hop differs: it connects
  to the service's internal address inside the deployment's network, never through NAT or the
  internet.
- Only the service's internal hostname reaches it, and only on the service's declared ports.
  Each DNS answer for that name must fall inside the address range the service can have (the
  cluster's Service range, the compute cluster's overlay, or the local deployment network).
  Every other private address stays refused, whatever is linked: other services, the nodes,
  loopback, link-local and cloud metadata addresses.
- Links require `egress("internet")`, and only Containers with a declared port can be linked.
  A link is checked when the stack is planned.
- Links are declared in the stack, reviewed with it, and shown at setup. Sandbox code and
  runtime policies cannot add links.

## Runtime policies narrow links

Links are a ceiling. A sandbox's runtime policy decides which of them a given sandbox uses:

| Policy | Links reachable |
| --- | --- |
| `internet` | every declared link |
| `allow: [..., "link:api"]` | only the links the list names as `link:<resource-id>` |
| `allow: []` | none |

`link:<resource-id>` takes no port; an id the stack does not link matches nothing.

## Bindings

Every command a sandbox runs (`runCommand`, `runInteractive`, `startJob`) receives each linked
service's binding as `ALIEN_<ID>_BINDING`, holding the service's internal URLs only, so code in
the sandbox reads the URL the same way a linked container does. A command whose own `env` sets
one of these names is refused.

The sandbox's own entrypoint, the image process started at create, does not receive them: it
starts before any command and its environment is fixed by the image. Read the binding from a
command, or pass the URL to the sandbox yourself.

The parent reads the same URL with `sandbox.linkUrl("api", 8080)` (Rust:
`Sandbox::link_url`), which answers `null` for a service the sandbox does not link.

Adding a link needs the parent to use an SDK from the release that added links. An older SDK
refuses a linked sandbox's binding on the cloud platforms; locally it ignores the links, so
commands do not receive their bindings.

## Platforms

| Platform | Links | How the gateway reaches the service |
| --- | --- | --- |
| Kubernetes | supported | The Service's cluster name, within the cluster's Service range. The gateway's NetworkPolicy opens only the linked pods and ports. |
| AWS | supported | The container's overlay name `<id>.svc`, within the compute cluster's overlay range. Only the gateway gets a path to the linked container's ports. |
| Local (`alien dev`) | supported | `<id>.svc` on the local deployment network (see below). |
| Google Cloud | not available yet | Planning refuses links with this reason. |
| Azure | supported | The linked container gets an internal load balancer with a static private address in the deployment's private subnet, and the gateway reaches the service at that one address. |

Planning also refuses links while the deployment installs a gateway image that predates them.
Stacks without links are unchanged on every platform: the same templates, the same gateway
configuration and the same bindings.

### Limits

- One sandbox resource links at most 16 services, and all sandboxes of a deployment together
  link at most 16 distinct services.
- A linked service declares at most 16 ports, and none of them may be port 53: the gateway
  answers DNS itself. Planning refuses other links with the reason.
- A deployment whose Operator predates links does not take a release that declares them (or a
  policy naming `link:<resource-id>`): the release is refused with the Operator version it
  needs, and the deployment keeps its current release.
- On a compute cluster whose control plane predates links, updating the egress gateway with
  links fails until the cluster is updated, instead of running the gateway without them.

### Removing a link

Removing a link from the stack removes it from the gateway's configuration on the next
heartbeat. New connections to the service are refused from then on, and tunnels already open
to it are closed by the gateway's periodic check, within about 30 seconds.

### Azure

These limits apply on Azure:

- A linked container has exactly one port and no public endpoint: it joins one load balancer
  through one host port. Planning refuses other containers with the reason.
- Adding a link to a running container rolls its replicas onto one shared host port, then
  creates the load balancer.
- Removing the last link stops the gateway from reaching the container on the next heartbeat.
  The idle internal load balancer stays until the container is replaced.
- Inside the deployment's own virtual network, the linked container's host port is reachable
  like that of any load-balanced container. Deployments with internet sandboxes refuse
  virtual network peering, and sandboxes reach the container only through the gateway.

### Local development

A local sandbox with links runs behind its own egress gateway container. Each of its sessions
sits alone on its own internal Docker network, which the gateway joins, so the gateway is the
session's only way out: to the internet and to the linked services. A local sandbox without
links keeps today's shared network and has no gateway.

- Each running session of a linked sandbox uses one Docker network, drawn from the daemon's
  address pools. With Docker's default pools that allows about 30 such sessions at once; when
  they run out, creating a session fails with an error naming the `default-address-pools`
  setting of the Docker daemon, which raises the limit.
- A session's gateway token lasts 24 hours. A linked local session that runs longer loses its
  outbound access; create a new session.
- The gateway's signing key exists only in the `alien dev` process. A restart removes every
  session, gateway and session network of the previous run.
- Tested on Linux Docker Engine. Docker Desktop on macOS is untested.
