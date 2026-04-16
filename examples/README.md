# Alien Examples

Each example is a self-contained template you can initialize with `alien init`.

| Template | Description | Language |
|----------|-------------|----------|
| [remote-worker-ts](./remote-worker-ts) | Execute tool calls in your customer's cloud. The AI worker pattern. | TypeScript |
| [basic-function-ts](./basic-function-ts) | The simplest Alien function, in TypeScript. | TypeScript |
| [basic-function-rs](./basic-function-rs) | The simplest Alien function, in Rust. | Rust |
| [data-connector-ts](./data-connector-ts) | Query private databases behind the customer's firewall. | TypeScript |
| [event-pipeline-ts](./event-pipeline-ts) | Process events from queues, storage changes, and cron schedules. | TypeScript |
| [webhook-api-ts](./webhook-api-ts) | Receive webhooks and expose an API inside the customer's cloud. | TypeScript |

## Getting started

```bash
# Install Alien
curl -fsSL https://alien.dev/install | sh

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
- [Stacks](https://alien.dev/docs/stacks) -- functions, storage, queues, vaults
