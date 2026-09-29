# Alien

[![X (formerly Twitter) Follow](https://img.shields.io/twitter/follow/alien)](https://x.com/alien)
[![GitHub Release](https://img.shields.io/github/v/release/alienplatform/alien)](https://github.com/alienplatform/alien/releases)
[![Slack](https://img.shields.io/badge/Slack-Join-4A154B?logo=slack&logoColor=white)](https://alien.dev/slack)

**Infrastructure for software outside your cloud.**

Some of your product has to run where you don't control the infrastructure: a customer's AWS, Google Cloud or Azure account, their Kubernetes cluster, or a site with no internet access at all. Alien runs that piece there (the data plane) and keeps it connected to your control plane: releases roll out on their own, logs come back, and your backend can call into it, all over connections the environment opens outbound.

The manager is the center of it. Run it yourself from this repository, or let [alien.dev](https://alien.dev) run it for you. The same `alien` CLI works with both.

## Quickstart

Install the CLI:

```bash
curl -fsSL https://alien.dev/install | sh
```

### With alien.dev

```bash
alien login
alien init && cd alien
alien dev        # run the app locally, no cloud account needed
alien release    # build and publish a release
alien onboard acme --platforms kubernetes
```

The [Quickstart](https://alien.dev/docs/quickstart) walks through it.

### With your own manager

Start a manager. It prints an admin API key the first time:

```bash
docker run -d --name alien-manager -p 8080:8080 -v alien-data:/data \
  -e BASE_URL=http://localhost:8080 \
  ghcr.io/alienplatform/alien-manager
docker logs alien-manager
```

Point the CLI at it, release your app, and onboard a customer:

```bash
alien login --manager http://localhost:8080 --token ax_admin_...
alien release
alien onboard acme --platforms kubernetes --secret-input accessToken=...
```

`alien onboard` prints a token for the customer and the commands their Kubernetes admin runs once. Their cluster pulls the chart and images from your manager with that token, which stays out of shell history:

```bash
read -rs ALIEN_TOKEN  # paste acme's token, then Enter
printf '%s' "$ALIEN_TOKEN" | helm registry login manager.example.com --username acme --password-stdin

printf 'management:\n  token: %s\n' "$ALIEN_TOKEN" | \
helm install data-plane oci://manager.example.com/charts/data-plane \
  --namespace data-plane --create-namespace \
  --set management.name=acme \
  --values values.yaml \
  --values -
```

For real customers, run the manager where their clusters can reach it over HTTPS: with the [Helm chart](infra/helm/alien-manager/) on Kubernetes, the [Terraform module](infra/aws-ecs-manager/) on Amazon ECS, or `docker run` on any machine. See [Self-hosting](https://alien.dev/docs/self-hosting).

## How it works

```
  Your cloud                               Customer environment
 ┌──────────────────────────┐             ┌──────────────────────────────┐
 │  alien CLI    your       │             │  Operator                    │
 │      │        backend    │  outbound   │   ├─ deploys each release    │
 │      ▼          │        │   HTTPS     │   ├─ ships logs and traces   │
 │   Manager ◀─────┘ ◀──────┼─────────────┼── ├─ relays tunnel requests  │
 │      │                   │             │   └─ updates itself          │
 │      ▼                   │             │                              │
 │  OpenTelemetry backend   │             │  Your containers, storage    │
 └──────────────────────────┘             └──────────────────────────────┘
```

Alien deploys in two ways:

- **Push.** The customer grants a narrowly scoped role in their cloud account, and the manager deploys through the cloud's APIs.
- **Pull.** The customer runs the Operator: a Helm chart on Kubernetes, or a container in their cloud. It connects outbound to the manager, fetches releases and deploys them locally. Nothing listens for inbound connections.

Both give you the same things: releases, heartbeats, logs and commands. See [How Alien works](https://alien.dev/docs/how-alien-works).

## Define your app

```typescript
import * as alien from "@alienplatform/core"

// The customer's S3-compatible bucket, supplied at install time.
const objects = new alien.Storage("objects").build()

const api = new alien.Container("api")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "api" } })
  .cpu(0.5)
  .memory("512Mi")
  .tunnel(8080) // reachable from your backend through the manager
  .link(objects)
  .permissions("api")
  .build()

export default new alien.Stack("data-plane")
  .platforms(["kubernetes"])
  .add(objects, "frozen")
  .add(api, "live")
  .permissions({ profiles: { api: { objects: ["storage/data-read", "storage/data-write"] } } })
  .build()
```

The same resources map to each platform's native services: Storage becomes S3, Google Cloud Storage, Azure Blob Storage, or an S3-compatible store (MinIO, Ceph) on Kubernetes. Workers, queues, key-value stores and vaults work the same way. See [Infrastructure](https://alien.dev/docs/infrastructure).

## After one install

Once a customer installs, you don't need access to their environment again:

- **Releases.** `alien release` rolls out to every deployment. The Operator also updates itself to the version the manager runs.
- **Images.** Clusters pull images through the manager with their deployment token. No registry credentials to hand out.
- **Logs and traces.** Deployments send OpenTelemetry to the manager, which forwards it to your backend: Datadog, Grafana, Honeycomb, Axiom, Coralogix, or any OTLP endpoint. `alien logs --deployment acme/acme` shows recent logs without one.
- **Tunnels.** Your backend calls a container inside any deployment through the manager, over the Operator's outbound connection. Request and response bodies stream in both directions, and the app's own `Authorization` header passes through:

  ```bash
  curl https://manager.example.com/v1/deployments/acme/tunnels/api/objects \
    -H "Proxy-Authorization: Bearer ax_tunnel_..." \
    -H "Authorization: Bearer <your app's token>"
  ```

  `alien tokens create --tunnel` makes a token that can only call tunnels, optionally for a single customer. Revoke it with `alien tokens revoke`.
- **Commands.** Invoke handlers inside a deployment from your backend with [Remote Commands](https://alien.dev/docs/commands).

## Air-gapped environments

Sites with no connection to your manager get the same app in a file. Package a release, carry the bundle in, and apply it with `alien-deploy`, which checks the manager's signature, pushes the images to the site's registry and installs or updates the chart:

```bash
alien onboard site-7 --platforms kubernetes --airgapped   # prints the bundle key
alien airgap bundle site-7/site-7 -o site-7.tar

# inside the site
alien-deploy airgap apply site-7.tar --registry registry.internal/vendor -f values.yaml \
  --trusted-key ed25519:...   # first install only; later bundles must match it
alien-deploy airgap status -n data-plane -o status.tar

# back with you
alien airgap import status.tar --deployment site-7/site-7
```

`airgap import` records the deployment's state and its logs in the manager, so `alien deployments ls` and `alien logs` cover those sites too. See [Air-gapped deployments](https://alien.dev/docs/deploying/air-gapped).

## Least-privilege permissions

Alien derives the permissions each part needs from the stack definition:

- **Provisioning.** The customer's admin sets up the environment once with their own credentials. Alien never holds these.
- **Management.** What Alien uses day to day. Frozen resources only get health checks. Live resources can be updated, but management never includes data access.
- **Application.** What your code can reach, as declared in permission profiles. `storage/data-read` becomes `s3:GetObject` on AWS, `storage.objects.get` on Google Cloud and the matching Azure role.

See [Permissions](https://alien.dev/docs/permissions) and [Frozen and live](https://alien.dev/docs/frozen-and-live).

## Examples

- [kubernetes-data-plane](examples/kubernetes-data-plane): a Rust object service in customer Kubernetes clusters, with S3-compatible storage and a tunnel
- [remote-worker-ts](examples/remote-worker-ts): tool execution inside the customer's cloud for an AI agent
- [data-connector-ts](examples/data-connector-ts): query private databases without sharing credentials
- [webhook-api-ts](examples/webhook-api-ts): an API inside the customer's network

More in [examples/](examples/).

## Repository

| Path | What it is |
|------|------------|
| [`crates/alien-cli`](crates/alien-cli) | The `alien` CLI |
| [`crates/alien-manager`](crates/alien-manager) | The manager |
| [`crates/alien-operator`](crates/alien-operator) | The Operator that runs in pull-mode environments |
| [`crates/alien-deploy-cli`](crates/alien-deploy-cli) | `alien-deploy`, run by a customer's admin |
| [`packages/`](packages) | TypeScript SDKs |
| [`infra/`](infra) | Helm chart and Terraform for running the manager, and cloud modules it uses |

## Documentation

- [Quickstart](https://alien.dev/docs/quickstart)
- [How Alien works](https://alien.dev/docs/how-alien-works)
- [Self-hosting](https://alien.dev/docs/self-hosting)
- [Kubernetes](https://alien.dev/docs/deploying/kubernetes)
- [Tunnels](https://alien.dev/docs/tunnels)
- [Observability](https://alien.dev/docs/observability)
- [Remote commands](https://alien.dev/docs/commands)

## Community

- [Slack](https://alien.dev/slack): help and feedback
- [GitHub Issues](https://github.com/alienplatform/alien/issues): bugs and feature requests
- [X](https://x.com/alien): updates
