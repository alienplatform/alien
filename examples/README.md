# Alien Examples

Every example here is a real, runnable Alien project. [Alien](https://alien.dev) lets you ship your product into your customers' AWS, GCP, Azure, or Kubernetes environments and keep it fully managed -- these examples show what that looks like in practice, from a single worker to a full application.

## Templates

Scaffold any of these with `alien init <name>` (or run `alien init` with no arguments to pick interactively):

| Template | Description | Language |
|----------|-------------|----------|
| [remote-worker-ts](./remote-worker-ts) | Execute tool calls in your customer's cloud. The AI worker pattern. | TypeScript |
| [basic-worker-ts](./basic-worker-ts) | The simplest Alien worker, in TypeScript. | TypeScript |
| [basic-worker-rs](./basic-worker-rs) | The simplest Alien worker, in Rust. | Rust |
| [data-connector-ts](./data-connector-ts) | Query private databases behind the customer's firewall. | TypeScript |
| [event-pipeline-ts](./event-pipeline-ts) | Process events from queues, storage changes, and cron schedules. | TypeScript |
| [webhook-api-ts](./webhook-api-ts) | Receive webhooks and expose an API inside the customer's cloud. | TypeScript |
| [nextjs-app](./nextjs-app) | Deploy a Next.js app as a single container in the customer's cloud. | TypeScript |
| [github-agent](./github-agent) | Full app: a GitHub integration with a Next.js dashboard as the control plane. | TypeScript |

## Full applications

Larger examples to read and adapt. Clone the repo and run them from their directories:

| Example | Description | Language |
|---------|-------------|----------|
| [byoc-database](./byoc-database) | A zero-disk vector database: stateless containers coordinating through object storage. | Rust |
| [full-stack-microservices](./full-stack-microservices) | A support desk app for Kubernetes: gateway, dashboard, API, worker, scheduler, Postgres, Redis. | TypeScript |
| [endpoint-agent](./endpoint-agent) | A daemon for employee devices with encrypted local storage, managed over commands. | Rust |

## Getting started

```bash
# Install Alien
curl -fsSL https://alien.dev/install | sh
export PATH="$HOME/.local/bin:$PATH"

# Scaffold a template
alien init remote-worker-ts
cd remote-worker-ts

# Start local development -- no cloud account needed
alien dev
```

## Learn more

- [Quickstart guide](https://alien.dev/docs/quickstart) -- build a worker, test locally, send remote commands
- [How Alien Works](https://alien.dev/docs/how-alien-works) -- stacks, isolated areas, push vs pull
- [Patterns](https://alien.dev/docs/patterns) -- remote worker, control/data plane, full app
- [Local Development](https://alien.dev/docs/local-development) -- `alien dev` reference
- [Remote Commands](https://alien.dev/docs/commands) -- invoke code on remote deployments
- [Stacks](https://alien.dev/docs/stacks) -- workers, storage, queues, vaults
