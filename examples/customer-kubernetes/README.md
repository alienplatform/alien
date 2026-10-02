# Run your service in your customers' Kubernetes clusters

Keep your control plane in your cloud, and run one service inside each customer's Kubernetes cluster: on-prem, in their cloud account, or on a network with no internet at all. Customers install it once. You ship, watch and call every copy from a manager you run.

```
 Your cloud                                Customer clusters
┌────────────────────────────────┐        ┌─────────────────────────────────────────┐
│ Control plane ──▶ Manager      │◀═══════│ customer-1  Operator ─▶ files ─▶ S3     │
│                   releases,    │◀═══════│ customer-2  Operator ─▶ files ─▶ MinIO  │
│                   images, logs,│        └─────────────────────────────────────────┘
│                   requests     │ by hand┌─────────────────────────────────────────┐
│                                │◀─ ─ ─ ▶│ customer-3  air-gapped                  │
│                                │        └─────────────────────────────────────────┘
└────────────────────────────────┘
```

## What you get

1. **Run your manager.** One open-source container in your cloud. It holds your releases, serves the chart and images, and collects logs.
2. **Customers install once.** One `helm install`. Their cluster connects out to your manager; nothing connects in.
3. **Every release rolls out.** Each `alien release` updates every cluster. No `helm upgrade`, no registry credentials to hand out.
4. **Logs come back.** Each cluster's logs reach your OpenTelemetry backend (Datadog, Grafana, Honeycomb, …), tagged with the customer.
5. **Call it by customer.** Your control plane calls the service in any cluster through the manager, with its own authentication passing through.
6. **Air-gapped sites too.** Clusters with no connection get the same releases as signed files carried in, and send their logs back the same way.

You can run all of it on your laptop in about half an hour: a local cluster plays three customers, one of them air-gapped.

## How it works

**The manager** is your side: one container (`ghcr.io/alienplatform/alien-manager`) with its state in a volume. It keeps your releases, decides which one each customer runs, serves the chart and images each cluster pulls with its own token, forwards logs to your OpenTelemetry backend, and forwards your control plane's requests to a given cluster.

**The Operator** is the customer's side. The chart installs it next to your service. It keeps one outbound HTTPS connection to the manager, deploys the release it should run, sends logs, and carries your requests to the service. It updates itself when you upgrade the manager.

**The service** is your code. This one is a small files service in Rust that keeps its data in the customer's own S3-compatible bucket (Amazon S3, MinIO, Ceph). [`alien.ts`](alien.ts) describes it:

```ts
const bucket = new alien.Storage("bucket").build() // the customer's bucket, named at install time

const api = new alien.Container("api")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "files" } })
  .tunnel(8080) // your control plane calls port 8080 through the manager, nobody else can
  .link(bucket)
  .permissions("api")
  .build()
```

[`src/main.rs`](src/main.rs) serves `PUT`, `GET` and `DELETE /files/{key}` and `GET /files?prefix=`, streaming to and from the bucket it gets from Alien's bindings. It checks a per-customer `accessToken` on every request, so the service keeps its own authentication end to end.

## Try it on your machine

A local [kind](https://kind.sigs.k8s.io) cluster plays your customers. `customer-1` and `customer-2` are connected; `customer-3` is air-gapped. Your laptop plays you, the vendor, and also the customers' admins.

### 1. Install the tools

You need Docker (Docker Desktop, OrbStack or Docker Engine), and:

| Tool | For | Install |
|---|---|---|
| `alien` | you, the vendor | `curl -fsSL https://alien.dev/install \| sh` |
| `alien-deploy` | the air-gapped customer's admin | `curl -fsSL https://alien.dev/install \| sh -s -- --binary alien-deploy` |
| `kind`, `kubectl`, `helm` (3.14 or later) | the local cluster and the customers' installs | [kind](https://kind.sigs.k8s.io/docs/user/quick-start/#installation), [kubectl](https://kubernetes.io/docs/tasks/tools/), [helm](https://helm.sh/docs/intro/install/) |
| Rust, [Zig](https://ziglang.org/download/) and `cargo-zigbuild` | building the example service for Linux | [rustup](https://rustup.rs), then `cargo install cargo-zigbuild` |
| `jq`, `openssl` | reading JSON output and making tokens in the steps below | your package manager |

Then get the example:

```bash
git clone https://github.com/alienplatform/alien.git
cd alien/examples/customer-kubernetes
npm install   # @alienplatform/core, which alien.ts is written with
```

Run every command below from this directory.

### 2. Create the customers' cluster

```bash
./local/cluster.sh up
```

This creates a kind cluster called `alien-demo` (kubectl context `kind-alien-demo`), S3-compatible storage in it with a bucket for each customer, and a registry at `localhost:5001` that plays the air-gapped customer's own registry. [`local/cluster.sh`](local/cluster.sh) explains each part.

Every `kubectl` and `helm` command in this guide names the context explicitly, so none of them touch another cluster your kubeconfig may point at.

### 3. Run your manager

Choose the manager's admin API key, then start the manager with it:

```bash
ADMIN_KEY=ax_admin_$(openssl rand -hex 16)

docker run -d --name alien-manager --network kind -p 127.0.0.1:8080:8080 \
  -v alien-manager-data:/data \
  -e BASE_URL=http://alien-manager:8080 \
  -e ALIEN_ADMIN_TOKEN=$ADMIN_KEY \
  ghcr.io/alienplatform/alien-manager

until curl -sf http://localhost:8080/health >/dev/null; do sleep 1; done   # wait until it's up
```

- `BASE_URL` is the address customer clusters use to reach the manager. Here it's the container's name on kind's network. In production it's your manager's public HTTPS URL.
- Your laptop reaches the same manager at `localhost:8080`, published only on this machine.
- `/data` holds all of its state: releases, images, deployments and keys.
- Without `ALIEN_ADMIN_TOKEN`, the first start generates a key and prints it once (`docker logs alien-manager`).

Connect the `alien` CLI:

```bash
alien login --manager http://localhost:8080 --token $ADMIN_KEY
alien whoami
```

From now on, every `alien` command talks to your manager.

### 4. Release the service

```bash
alien release
```

This builds the container for Linux (the first build takes several minutes) and pushes the image into your manager, which stores it and records the release. You don't need a registry of your own.

### 5. Onboard two customers

Each customer gets an access token that your control plane will send to their copy of the service. Make one per customer, then onboard them:

```bash
CUSTOMER_1_ACCESS=$(openssl rand -hex 24)
CUSTOMER_2_ACCESS=$(openssl rand -hex 24)

alien onboard customer-1 --platforms kubernetes \
  --secret-input accessToken=$CUSTOMER_1_ACCESS --json > customer-1.json
alien onboard customer-2 --platforms kubernetes \
  --secret-input accessToken=$CUSTOMER_2_ACCESS --json > customer-2.json

jq . customer-1.json
```

`onboard` registers the customer and returns what to send their Kubernetes admin:

- `token`: their cluster's token. The cluster uses it to pull the chart and images from your manager, report status and send logs, and it can only do that as this customer.
- `helm.command`: the commands the admin runs once.
- `helm.values`: a `values.yaml` for their bucket, which the admin fills in.

Without `--json`, `onboard` prints the same as ready-to-send instructions.

### 6. Install, as each customer's admin

This is what each customer's admin runs, once. Compared with `helm.command`, two things change on your laptop: the commands use `localhost:8080`, where your laptop reaches the manager (a real admin uses the URL `onboard` printed), and they name the local cluster's context.

```bash
for n in 1 2; do
  token=$(jq -r .token customer-$n.json)
  printf '%s' "$token" |
    helm registry login localhost:8080 --insecure --username customer-$n --password-stdin
  helm install files oci://localhost:8080/charts/files --plain-http \
    --kube-context kind-alien-demo --namespace customer-$n --create-namespace \
    --set management.name=customer-$n \
    --set-string management.token="$token" \
    --values local/customer-$n.yaml \
    --wait --timeout 5m
done
```

Each install starts the Operator, which connects to your manager, fetches the release and deploys the service. In a minute or two both customers show as `running`:

```bash
alien deployments ls
kubectl --context kind-alien-demo -n customer-1 get pods
```

In each namespace, `files-...` is the Operator and `api-...` is your service. The cluster pulled the chart, the Operator and your service's image from your manager. It never talked to another registry.

### 7. Call a customer's service from your control plane

Your control plane gets a token that can only call services through the manager:

```bash
TUNNEL_TOKEN=$(alien tokens create --tunnel --json | jq -r .token)
```

Then it calls any customer's service by name, at `/v1/deployments/<customer>/tunnels/<container>/<path>`:

```bash
MANAGER=http://localhost:8080

curl -s -w '\n' -X PUT --data-binary 'quarterly numbers' \
  $MANAGER/v1/deployments/customer-1/tunnels/api/files/reports/q3.txt \
  -H "Proxy-Authorization: Bearer $TUNNEL_TOKEN" \
  -H "Authorization: Bearer $CUSTOMER_1_ACCESS"

curl -s -w '\n' $MANAGER/v1/deployments/customer-1/tunnels/api/files/reports/q3.txt \
  -H "Proxy-Authorization: Bearer $TUNNEL_TOKEN" \
  -H "Authorization: Bearer $CUSTOMER_1_ACCESS"
```

- The manager checks `Proxy-Authorization` and forwards the request over customer-1's Operator connection.
- `Authorization` reaches the service untouched: the service checks its own token. Send customer-2's token to customer-1 and the service answers 401.
- Bodies stream both ways, so large files never sit in memory on the way.

The file is now in customer-1's bucket, in their storage.

### 8. Ship an update

Change the service's health check to report a version. In `src/main.rs`, change the `/health` route to:

```rust
.route("/health", get(|| async { "ok, v2" }))
```

Then release:

```bash
alien release
```

Both Operators pick up the new release on their next check-in and roll it out. Nobody runs `helm upgrade`, and you never touch the clusters:

```bash
alien deployments ls        # update-pending, then running on the new release
curl -s -w '\n' $MANAGER/v1/deployments/customer-2/tunnels/api/health \
  -H "Proxy-Authorization: Bearer $TUNNEL_TOKEN"     # ok, v2
```

### 9. Read the logs

The service logs JSON to stdout. Each Operator collects the logs of its customer's service and sends them to your manager:

```bash
alien logs --deployment customer-1/customer-1 --since 15m
```

```
2026-09-30T08:28:41.543Z INFO  api stored file
2026-09-30T08:28:41.599Z WARN  api rejected request without a valid access token
2026-09-30T08:30:18.883Z INFO  api bucket ready
```

In production, point the manager at your OpenTelemetry backend and the logs land there too, tagged with `alien.deployment_id`. See [Observability](https://alien.dev/docs/observability).

### 10. Add an air-gapped customer

Some sites have no network path to your manager at all. They still run the same service and get every release, as files someone carries in. Here's the loop, for a site called `customer-3`:

```
 online machine                        the folder                    inside the site
 alien-deploy sync   ────▶   customer-3-sync/ to-site   ────▶   alien-deploy sync
 (sends reports,                                               (verifies, installs,
  downloads the next         customer-3-sync/ from-site          writes a report)
  signed update)     ◀────                                ◀────
```

- **Online**, on any machine that reaches your manager, `alien-deploy sync` sends the site's reports to the manager and downloads the next update into a folder.
- **Inside the site**, the admin runs `alien-deploy sync` again. It checks the update's signature, pushes the images into the site's own registry, installs or upgrades the chart, waits until the service runs, and writes a report: the deployment's status and the logs collected since the last trip.
- The folder goes back and forth on whatever the site allows: a USB drive, a data diode, a one-way transfer.

Updates are signed by your manager, and the site checks every one against your key. After the first update, only image layers the site doesn't have are carried. The Operator at the site never connects anywhere; it reads the updates `alien-deploy sync` hands it.

Onboard the site. With `--airgapped`, `onboard` registers it right away and prints what to send its admin:

```bash
CUSTOMER_3_ACCESS=$(openssl rand -hex 24)
alien onboard customer-3 --platforms kubernetes --airgapped \
  --secret-input accessToken=$CUSTOMER_3_ACCESS --json > customer-3.json
jq . customer-3.json
```

- `token` is the site's token. It stays on the online machine and can only download this site's updates and send its reports.
- `bundleSigningKey` is your manager's key (`ed25519:...`). The site trusts updates signed with it. Give it to the admin separately from the token, for example over the phone, so a tampered update can't also carry a forged key.
- `command` is how the admin starts, on the online machine.

**Online: download the first update.** As before, use `localhost:8080` where the command says `http://alien-manager:8080`:

```bash
mkdir -p airgap && cd airgap
alien-deploy sync --token $(jq -r .token ../customer-3.json) --manager http://localhost:8080
```

This creates `customer-3-sync/` with the update in `to-site/`. The first update carries every image, the chart and `alien-deploy` builds for the site, so it's large; later ones only carry what changed.

**Inside the site: install.** Carry `customer-3-sync/` in. The site admin runs `alien-deploy sync` from the directory that holds it, the first time with the site's registry, their Helm values and your key:

```bash
alien-deploy sync \
  --registry localhost:5001/vendor --insecure-registry \
  --namespace customer-3 --values ../local/customer-3.yaml \
  --kube-context kind-alien-demo \
  --trusted-key $(jq -r .bundleSigningKey ../customer-3.json)
```

It verifies the update, pushes the images to `localhost:5001`, installs the chart, waits for the service to run, and writes a report into `customer-3-sync/from-site/`. It remembers the registry, namespace, values and context, and the install remembers your key: later syncs need no options, and an update signed with any other key is refused.

Your laptop reaches the manager and the cluster at once, so this run also sent the report straight back. At a real site, the admin carries the folder out and runs `alien-deploy sync` online, which sends the report and downloads the next update in the same trip.

The site now shows up like any other customer:

```bash
alien deployments ls
alien logs --deployment customer-3/customer-3 --since 15m
```

**Every release after that.** Release as usual; the site gets it on the next trip:

```bash
cd ..
# change "ok, v2" to "ok, v3" in src/main.rs
alien release
cd airgap && alien-deploy sync
```

On your laptop, that one command does both halves. The update is a few megabytes: only the layers that changed.

- `alien-deploy sync --dry-run` shows what an update will change before it installs anything.
- `alien-deploy rollback` returns the site to the release it ran before. To keep it there, pin it on your side (`alien deployments pin customer-3/customer-3 <release>`); otherwise the next trip brings the latest release again.
- When the site already runs the latest release, a trip still brings back a small signed update: it tells the site's Operator which telemetry your manager received, so the Operator can free it.

See [Air-gapped deployments](https://alien.dev/docs/deploying/air-gapped) for the details.

### 11. Clean up

```bash
cd ..
docker rm -f alien-manager && docker volume rm alien-manager-data
./local/cluster.sh down
rm -rf airgap customer-1.json customer-2.json customer-3.json
```

## Going to production

**Run the manager** where your customers' clusters can reach it over HTTPS: `docker run` on a VM behind a TLS proxy, the [Helm chart](https://github.com/alienplatform/alien/tree/main/infra/helm/alien-manager) on your Kubernetes, or the [Terraform module](https://github.com/alienplatform/alien/tree/main/infra/aws-ecs-manager) on Amazon ECS. Set `BASE_URL` to its public URL, keep `/data` on a persistent, backed-up volume, and set its telemetry to your OpenTelemetry backend. See [Self-hosting](https://alien.dev/docs/self-hosting).

**Onboard each customer** with `alien onboard`, and send their admin what it prints: the token, the two Helm commands and the `values.yaml` for their bucket. Their cluster needs outbound HTTPS to your manager's URL, nothing else. See [Kubernetes](https://alien.dev/docs/deploying/kubernetes).

**Call the services** from your control plane with a tunnel token: `alien tokens create --tunnel`, optionally limited to one customer with `--customer`. See [Tunnels](https://alien.dev/docs/tunnels).

```ts
const res = await fetch(
  `https://manager.example.com/v1/deployments/${customer}/tunnels/api/files/${key}`,
  {
    method: "PUT",
    body: file,
    headers: {
      "Proxy-Authorization": `Bearer ${process.env.ALIEN_TUNNEL_TOKEN}`,
      Authorization: `Bearer ${accessTokens[customer]}`,
      "If-None-Match": "*",
    },
  },
)
```

**Air-gapped sites** need `alien-deploy`, a container registry of their own, and a way to move a folder in and out. See [Air-gapped deployments](https://alien.dev/docs/deploying/air-gapped).

**Release** whenever you like. `alien release` reaches every connected customer within a minute, and each air-gapped site on its next trip. To stage releases, use [release channels and pins](https://alien.dev/docs/releases).
