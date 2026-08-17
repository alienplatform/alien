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
| [customer-models-ts](./customer-models-ts) | Let each customer connect models from their cloud account. | TypeScript |
| [byob-storage-ts](./byob-storage-ts) | Provision customer-owned object storage and access it from an external SaaS backend. | TypeScript |
| [customer-keys-ts](./customer-keys-ts) | Encrypt data with a key controlled by each customer. | TypeScript |

Some repository directories are supporting projects or advanced source examples. `alien init`
only lists directories with a valid `template.toml`, so every option it shows can be scaffolded.

## Getting started

```bash
# Install Alien
curl -fsSL https://alien.dev/install | sh
export PATH="$HOME/.local/bin:$PATH"

# Create a project from a template
alien init my-app

# Start local development
cd my-app
alien dev
```

## Learn more

- [Quickstart guide](https://alien.dev/docs/quickstart) -- build a worker, test locally, send remote commands
- [How Alien Works](https://alien.dev/docs/how-alien-works) -- stacks, isolated areas, push vs pull
- [Patterns](https://alien.dev/docs/patterns) -- remote worker, control/data plane, full app
- [Local Development](https://alien.dev/docs/local-development) -- `alien dev` reference
- [Remote Commands](https://alien.dev/docs/commands) -- invoke code on remote deployments
- [Stacks](https://alien.dev/docs/stacks) -- workers, storage, queues, vaults
