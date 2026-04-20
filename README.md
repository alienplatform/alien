# Alien

Alien provides infrastructure to deploy and operate software inside your users' environments, while retaining centralized control over updates, monitoring, and lifecycle management.

## Why Alien?

Self-hosting works - *until someone starts paying for your software*.

Customers run it in their own environment, but they don't actually know how to operate it. They might change something small like Postgres version, environment variables, IAM, firewall rules, and things start failing. From their perspective, your product is broken. And even if the root cause is on their side, it doesn't matter... the customer is always right, you're still the one expected to fix it.

But you can't. You don't have access to their environment. You don't have real visibility. You can't run anything yourself. So you're stuck debugging a system you don't control, through screenshots and copy-pasted logs on a Zoom call. You end up responsible for something you don't control.

Alien provides a better model: **managed self-hosting**.

## Quickstart

Install the CLI:

```bash
curl -fsSL https://alien.dev/install | sh
```

Create a project and start developing:

```bash
alien init
cd my-project && pnpm dev
```

Follow the [Quickstart](https://www.alien.dev/docs/quickstart) guide to build an AI worker, test it locally, and deploy it — no cloud account needed to start.

Or [try it with Claude Code, Codex, or Cursor](https://www.alien.dev#prompt).

## Features

- **[AWS, GCP, and Azure support](https://www.alien.dev/docs/how-alien-works)** - Deploy to all major clouds. 
- **[TypeScript & Rust](https://alien.dev/docs/infrastructure/function/toolchains)** — First-class support for both. Python and arbitrary containers coming soon.
- **[Real-time Heartbeat](https://alien.dev/docs/how-alien-works)** — Know the instant a deployment goes down. 
- **[Auto Updates & Rollbacks](https://alien.dev/docs/releases)** — Push a release and every remote environment picks it up automatically. 
- **[Local-first Development](https://alien.dev/docs/local-development)** — Build and test on your machine. Local equivalents for every cloud resource.
- **[Cloud-agnostic Infrastructure](https://alien.dev/docs/infrastructure)** — Ship to AWS, GCP, and Azure customers without maintaining separate integrations. Alien maps a single API to each cloud's native services at deploy time.
- **[Remote Commands](https://alien.dev/docs/commands)** — Invoke code on remote deployments from your control plane. Zero inbound networking. Zero open ports. No VPC peering.
- **[Observability](https://alien.dev/docs/how-alien-works)** — Logs, metrics, and traces from every deployment. Full visibility without touching customer infrastructure.
- **[Least-privilege Permissions](https://alien.dev/docs/permissions)** — Alien derives the exact IAM permissions required to deploy and manage your app.

## What you can build

- **AI Worker** — Agent harness in your cloud, tool execution in theirs. Read files, run commands, query data — all local. ([example](examples/remote-worker-ts))
- **Data Connector** — Query Snowflake, Postgres, or any private database. No shared credentials, no exposed services. ([example](examples/data-connector-ts))
- **Browser Automation** — Headless browser inside their network. Navigate Jira, SAP, GitLab, on-prem wikis. Only results leave.
- **Security Outpost** — Scan IAM policies, storage, network configs from inside the perimeter. On a schedule or on-demand.
- **Cloud Actions** — API inside their network. Restart services, rotate credentials, react to infrastructure changes. ([example](examples/webhook-api-ts))

## How deployment works

### Push model

**Like sharing a Google Drive folder.** The customer grants least-privilege access to an isolated area in their cloud. You run `alien serve` on your infrastructure and it manages everything through cloud APIs (e.g. AWS `UpdateFunctionCode`). No network connection to their environment needed.

```bash
alien serve
```

```
                                              ╔═ Customer's Cloud ══════════════════╗
                                              ║                                     ║
                                              ║  Their databases, services, infra   ║
                                              ║                                     ║
╔═ alien serve ═══════════╗                   ║  ┌─ Isolated Area ──────────────┐   ║
║                         ║   cloud APIs      ║  │                              │   ║
║  Push updates    ───────╬───────────────────╬─▶│  ┏━━━━━━━━━━┓                │   ║
║  Collect telemetry ◀────╬───────────────────╬──│  ┃ Function ┃                │   ║
║  Run commands    ───────╬───────────────────╬─▶│  ┗━━━━━━━━━━┛                │   ║
║                         ║                   ║  │  ┏━━━━━━━━━━┓                │   ║
║                         ║                   ║  │  ┃ Storage  ┃                │   ║
╚═════════════════════════╝                   ║  │  ┗━━━━━━━━━━┛                │   ║
                                              ║  └──────────────────────────────┘   ║
                                              ║                                     ║
                                              ╚═════════════════════════════════════╝
```

### Pull model

**Like an app checking for updates.** For customers that can't or won't allow a cross-account IAM role, they can run `alien-agent` in their environment instead. It connects outbound to the Alien server, fetches releases, and deploys locally. No inbound connections, no open ports.

```bash
docker run ghcr.io/alienplatform/alien-agent \
  --sync-url https://alien.example.com \
  --sync-token <token> \
  --platform aws
```

```
                                              ╔═ Customer's Cloud ══════════════════╗
                                              ║                                     ║
                                              ║  Their databases, services, infra   ║
                                              ║                                     ║
╔═ alien serve ═══════════╗     outbound      ║  ┌─ Isolated Area ──────────────┐   ║
║                         ║      HTTPS        ║  │                              │   ║
║  Releases        ◀──────╬───────────────────╬──│── alien-agent                │   ║
║  Telemetry       ◀──────╬───────────────────╬──│──  ┏━━━━━━━━━━┓              │   ║
║  Commands        ◀──────╬───────────────────╬──│──  ┃ Function ┃              │   ║
║                         ║                   ║  │    ┗━━━━━━━━━━┛              │   ║
║                         ║                   ║  │    ┏━━━━━━━━━━┓              │   ║
╚═════════════════════════╝                   ║  │    ┃ Storage  ┃              │   ║
                                              ║  │    ┗━━━━━━━━━━┛              │   ║
                                              ║  └──────────────────────────────┘   ║
                                              ║                                     ║
                                              ╚═════════════════════════════════════╝
```

Both models give you the same capabilities: updates, telemetry, remote commands. See [Deployment Models](https://alien.dev/docs/deploying/deployment-models).

## Releases

Push a release and every environment updates automatically.

```bash
alien release
```

Builds your code, pushes artifacts, and creates a release. Every active deployment picks up the new version.

## One codebase, every cloud

Ship to AWS, GCP, and Azure customers without maintaining separate integrations. Alien maps your stack to each cloud's native services at deploy time.

```typescript
import * as alien from "@alienplatform/core"

const data = new alien.Storage("data").build()
const secrets = new alien.Vault("credentials").build()

const api = new alien.Function("api")
  .code({ type: "source", src: "./api", toolchain: { type: "typescript" } })
  .link(data)
  .link(secrets)
  .ingress("public")
  .build()

export default new alien.Stack("my-app")
  .add(api, "live")
  .add(data, "frozen")
  .add(secrets, "frozen")
  .build()
```

At deploy time, each resource maps to the cloud's native service:

```
  ┏━━━━━━━━━━━━┓                    ┏━━━━━━━━━━━━┓
  ┃  Function  ┃                    ┃  Storage   ┃
  ┗━━━━━┯━━━━━━┛                    ┗━━━━━┯━━━━━━┛
        │                                 │
        ├── AWS ───▶ Lambda               ├── AWS ───▶ S3
        ├── GCP ───▶ Cloud Run            ├── GCP ───▶ Google Cloud Storage
        └── Azure ─▶ Container App        └── Azure ─▶ Azure Blob Storage
```

The same applies to queues, vaults, and KV stores. One codebase, all clouds. Drop to native SDKs whenever you need to.

Each resource documents its [guarantees, limits, and platform-specific behavior](https://alien.dev/docs/infrastructure) so you know exactly what to expect across clouds.

## Remote commands

Invoke code inside the customer's environment from your control plane. Zero inbound networking, zero open ports.

Define a handler in the customer's environment:

```typescript
import { command, storage } from "@alienplatform/sdk"

const files = storage("files")

command("read-file", async ({ path }) => {
  const { data } = await files.get(path)
  return { content: new TextDecoder().decode(data) }
})
```

Invoke it from your backend:

```typescript
const result = await commands.invoke("read-file", {
  path: "report.csv"
})
```

See [Remote Commands](https://alien.dev/docs/commands).

## Least-privilege permissions

You're deploying to someone else's cloud. Every permission needs justification. Alien derives exactly the permissions needed from your stack definition — for AWS, GCP, and Azure.

```typescript
export default new alien.Stack("my-app")
  .add(data, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        data: ["storage/data-read", "storage/data-write"],
      },
    },
  })
  .build()
```

From this definition, Alien derives three layers of permissions:

**Provisioning** — Creates all resources during initial setup. The customer's admin runs `alien-deploy up` once with their own credentials. Alien never holds these permissions.

**Management** — What Alien uses day-to-day to manage the deployment:

- 🧊 **Frozen** resources: health checks only. No ability to modify, delete, or read data.
- 🔁 **Live** resources: push code, roll config, redeploy. But still no data access — Alien can call `lambda:UpdateFunctionCode` but never `s3:GetObject`. Management and data access are separate.

**Application runtime** — What the deployed code can access. Only what's declared in permission profiles. The `execution` profile above grants `storage/data-read` and `storage/data-write` on the `data` bucket — nothing else. No declaration, no access.

Permission sets are portable across clouds:

| | `storage/data-read` |
|---|---|
| AWS | `s3:GetObject`, `s3:ListBucket` |
| GCP | `storage.objects.get`, `storage.objects.list` |
| Azure | `Microsoft.Storage/.../blobs/read` |

For edge cases, define custom permission sets with cloud-specific actions:

```typescript
const assumeRole: PermissionSet = {
  id: "assume-role",
  platforms: {
    aws: [{
      grant: { actions: ["sts:AssumeRole"] },
      binding: { stack: { resources: ["*"] } }
    }]
  }
}
```

See [Permissions](https://alien.dev/docs/permissions) and [Frozen & Live](https://alien.dev/docs/frozen-and-live).

## Production deployment

**1. Generate a config template:**

```bash
alien serve --init   # creates alien-manager.toml
```

**2. Provision cloud resources for push-mode platforms** (optional — Terraform modules for [AWS](infra/aws/), [GCP](infra/gcp/), [Azure](infra/azure/)):

```hcl
module "alien_infra" {
  source = "github.com/aliendotdev/alien//infra/aws"

  name          = "my-project"
  principal_arn = aws_iam_role.manager.arn
}
```

Fill the Terraform outputs into `alien-manager.toml`.

**3. Run the server.** The server must be reachable over HTTPS — deployments and agents connect back to it.

```bash
docker run -d -p 8080:8080 \
  -v alien-data:/data \
  -v ./alien-manager.toml:/app/alien-manager.toml \
  -e BASE_URL=https://manager.example.com \
  ghcr.io/alienplatform/alien-manager
```

See the [Self-Hosting Guide](https://alien.dev/docs/self-hosting) for the full configuration reference and production checklist.

## Documentation

- [Quickstart](https://alien.dev/docs/quickstart) — build and deploy an AI worker
- [How Alien Works](https://alien.dev/docs/how-alien-works) — architecture and core concepts
- [Stacks](https://alien.dev/docs/stacks) — defining your infrastructure
- [Frozen and Live](https://alien.dev/docs/frozen-and-live) — the security/control tradeoff
- [Deployment Models](https://alien.dev/docs/deploying/deployment-models) — push vs pull
- [Remote Commands](https://alien.dev/docs/commands) — invoking code in customer environments
- [Permissions](https://alien.dev/docs/permissions) — least-privilege access control

## Community

- [Discord](https://alien.dev/discord) — get help and share feedback
- [GitHub Issues](https://github.com/alienplatform/alien/issues) — bug reports and feature requests
- [X](https://x.com/alien) — updates and announcements
