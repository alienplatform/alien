# Alien Examples

Each example is a self-contained template you can initialize with `alien init`.

| Template | Description | Language |
|----------|-------------|----------|
| [basic-worker-ts](./basic-worker-ts) | The smallest remote worker, in TypeScript. | TypeScript |
| [basic-worker-rs](./basic-worker-rs) | The smallest remote worker, in Rust. | Rust |
| [ai-quickstart-ts](./ai-quickstart-ts) | Call the cloud AI service already available in a remote environment. | TypeScript |
| [remote-worker-ts](./remote-worker-ts) | Execute agent tool calls near private services and data. | TypeScript |
| [data-connector-ts](./data-connector-ts) | Query a private database without exposing it to the public internet. | TypeScript |
| [webhook-api-ts](./webhook-api-ts) | Receive webhooks and expose an API in a remote environment. | TypeScript |
| [event-pipeline-ts](./event-pipeline-ts) | Process queue, storage, and scheduled events. | TypeScript |
| [nextjs-app](./nextjs-app) | Deploy a complete Next.js application as a container. | TypeScript |
| [github-agent](./github-agent) | Build a GitHub integration agent with a Next.js dashboard. | TypeScript |
| [customer-kubernetes](./customer-kubernetes) | Run a service in your customers' Kubernetes clusters, including air-gapped ones, from a manager you host. | Rust |
| [customer-models-ts](./customer-models-ts) | Let each customer connect models from their cloud account. | TypeScript |
| [byob-storage-ts](./byob-storage-ts) | Provision customer-owned object storage and access it from an external SaaS backend. | TypeScript |
| [customer-keys-ts](./customer-keys-ts) | Encrypt data with a key controlled by each customer. | TypeScript |
| [ai-chatbot-ts](./ai-chatbot-ts) | Answer questions about a private Postgres with ambient cloud models. | TypeScript |
| [extensions/postgres-iam-auth](./extensions/postgres-iam-auth) | An extension that turns on IAM authentication for Alien's Postgres on every setup path. | Rust |
| [extensions/postgres-backup-retention](./extensions/postgres-backup-retention) | An extension that keeps Postgres backups for 30 days in every setup template. | Rust |
| [extensions/resource-tags](./extensions/resource-tags) | An extension that adds your tags to the resources Alien's controllers create. | Rust |
| [extensions/budget-alert](./extensions/budget-alert) | An extension that adds an AWS budget alert, a custom resource with no controller. | Rust |
| [extensions/dns-zone](./extensions/dns-zone) | An extension that adds a DNS zone, a custom resource with its own controllers. | Rust |

Some repository directories are supporting projects or advanced source examples. `alien init`
only lists directories with a valid `template.toml`, so every option it shows can be scaffolded.

The [custom operations example](./custom-operations) demonstrates a Rust Remote Operator
plugin with typed diagnostics, export throttling, and generated metadata.

[extensions/](./extensions) explains what each extension example shows, and
[dataplane-with-extension](./dataplane-with-extension) is a project whose `alien.ts` uses an
extension that lives beside it.

## Getting started

```bash
# Install Alien
curl -fsSL https://alien.dev/install | sh -s -- --login

# Create a project from a template
alien init

# Start local development
cd alien
alien dev
```

## Learn more

- [Quickstart guide](https://alien.dev/docs/quickstart) -- build a worker, test locally, send remote commands
- [How Alien Works](https://alien.dev/docs/how-alien-works) -- stacks, isolated areas, push vs pull
- [Patterns](https://alien.dev/docs/patterns) -- remote worker, control/data plane, full app
- [Local Development](https://alien.dev/docs/local-development) -- `alien dev` reference
- [Remote Commands](https://alien.dev/docs/commands) -- invoke code on remote deployments
- [Stacks](https://alien.dev/docs/stacks) -- workers, storage, queues, vaults
