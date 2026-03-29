# Alien

Deploy software to your customers' cloud — and keep it fully managed.

Every enterprise deal ends the same way: *"Can you deploy this into our cloud?"* The usual answer is shipping a Docker image or Helm chart and losing control. No auto-updates, no logs, no way to debug without a support call. Every customer runs a different version.

Alien deploys your app to remote environments — **AWS**, **GCP**, **Azure**, **Kubernetes**, or a **single VM** — and keeps it updated, monitored, and under your control.

## How It Works

Define your app in `alien.ts` — bring an existing container, add cloud resources, wire it together:

```typescript
import * as alien from "@alienplatform/core"

// Object storage — S3 on AWS, Cloud Storage on GCP, Blob Storage on Azure
const data = new alien.Storage("data").build()

// Bring your existing Docker image
const app = new alien.Container("app")
  .image("./Dockerfile")
  .port(3000)
  .ingress("public")
  .build()

export default new alien.Stack("my-saas")
  .add(app, "live")      // updated on every release, least-privilege permissions
  .add(data, "frozen")   // created once during setup, with elevated permissions
  .build()
```

No framework lock-in. Any language, any framework. `alien.ts` just describes what to deploy.

## Getting Started

```bash
curl -fsSL https://alien.dev/install | bash
```

### Start from a template

```bash
alien init
```

Pick an example project — a minimal agent, a vector database, a GitHub integration — and get a working `alien.ts` in seconds. Or write your own from scratch.

### Develop

```bash
alien dev
```

On first run, you'll be asked to sign up or log in. Then your stack builds and deploys locally with Docker, hot-reloading on code changes.

`alien dev` runs everything locally with Docker — fast iteration, no cloud credentials needed. For cloud deployment, use `alien deploy`.

### Release

```bash
alien release
```

Builds and pushes a release.

### Onboard a customer

```bash
alien onboard acme-corp
```

Outputs a deploy link. Send it to the customer's admin.

The admin sees a branded deploy page — your name, your logo. They install your auto-generated CLI and run the one-time setup:

```bash
curl -fsSL https://manager.alien.dev/install | bash
my-saas-deploy up --token dg_abc123... --platform aws
```

This grants limited cross-account access and provisions infrastructure (storage, networking, IAM) in the customer's cloud. The admin's involvement ends here.

### Push updates

From now on, every `alien release` automatically updates every customer. Only application code is updated — with minimal permissions. No admin involvement, no version drift.

---

## Cloud-Agnostic Bindings

Optional bindings that work on every platform. Use them, or use any existing libraries directly.

```typescript
import { storage, kv, ai } from "@alienplatform/bindings"

// S3 on AWS, Cloud Storage on GCP, Blob Storage on Azure, filesystem locally
const store = storage("data")
const report = await store.get("reports/latest.json")

// DynamoDB on AWS, Firestore on GCP, Table Storage on Azure, SQLite locally
const cache = kv("cache")
await cache.set("user:123", { name: "Alice", plan: "pro" })

// Bedrock on AWS, Vertex AI on GCP, Azure OpenAI on Azure
const summary = await ai().generate("Summarize this quarter's sales")
```

## Remote Commands

Invoke code on remote deployments. Zero inbound networking. Zero open ports.

```typescript
import { command } from "@alienplatform/bindings"

command("generate-report", async ({ startDate, endDate }) => {
  const data = await fetchData(startDate, endDate)
  return { report: aggregate(data) }
})
```

```bash
alien commands invoke \
  --deployment acme-corp \
  --command generate-report \
  --params '{"startDate": "2025-01-01"}'
```

---

## Standalone Mode

Alien is fully open-source. Run the entire system without an account:

```bash
alien-manager
```

Starts a self-contained manager with SQLite. Full functionality on Local and Kubernetes. Cloud platforms work for private-ingress functions. See the [Standalone Guide](user-guide/09-standalone.md).

## Documentation

- **[User Guide](user-guide/)** — from first deploy to production
- **[Internal Docs](docs/)** — architecture and contributor guide

## License

See [LICENSE](LICENSE) for details.
