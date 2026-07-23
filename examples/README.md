# Alien Examples

Each example is a self-contained template you can initialize with `alien init`.

| Template | Description | Language |
|----------|-------------|----------|
| [byob-storage-ts](./byob-storage-ts) | Provision customer-owned object storage and access it from an external SaaS backend. | TypeScript |
| [remote-worker-ts](./remote-worker-ts) | Execute tool calls in your customer's cloud. The AI worker pattern. | TypeScript |
| [basic-worker-ts](./basic-worker-ts) | The simplest Alien worker, in TypeScript. | TypeScript |
| [basic-worker-rs](./basic-worker-rs) | The simplest Alien worker, in Rust. | Rust |
| [data-connector-ts](./data-connector-ts) | Query private databases behind the customer's firewall. | TypeScript |
| [event-pipeline-ts](./event-pipeline-ts) | Process events from queues, storage changes, and cron schedules. | TypeScript |
| [webhook-api-ts](./webhook-api-ts) | Receive webhooks and expose an API inside the customer's cloud. | TypeScript |
| [nextjs-app](./nextjs-app) | Deploy a Next.js app as a single container in the customer's cloud. | TypeScript |

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
