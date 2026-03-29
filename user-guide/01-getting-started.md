# Getting Started

## Install

```bash
curl -fsSL https://alien.dev/install | bash
```

## Start from a template

```bash
alien init
```

Pick an example project:

- **minimal-cloud-agent** — the simplest possible setup: one function with a command handler
- **byoc-database** — a multi-container vector database with writer/reader separation
- **github-agent** — HTTP routes with Hono, vault access, command patterns
- **endpoint-agent** — a Rust agent for employee devices, monitors system activity

Or skip `alien init` and write `alien.ts` from scratch. See [Defining Your App](02-defining-your-app.md).

## Develop

```bash
alien dev
```

On first run, you'll be asked to sign up or log in. Then your stack builds and deploys locally with Docker, hot-reloading on code changes.

## Release

```bash
alien release
```

Builds your app and pushes a release. Any existing deployments pick up the new version automatically.

## Onboard your first customer

```bash
alien onboard acme-corp
```

This creates a deploy link:

```
Deployment group 'acme-corp' created.

Deploy link:
  https://my-saas.alien.dev/deploy#token=dg_abc123...
```

Send the link to the customer's admin. They see a branded deploy page — your name, your logo — install your auto-generated CLI, and run the one-time setup:

```bash
curl -fsSL https://manager.alien.dev/install | bash
my-saas-deploy up --token dg_abc123... --platform aws
```

This grants limited access and provisions infrastructure in the customer's cloud. The admin's involvement ends here. From now on, every `alien release` automatically updates their deployment.

## Next

- [Defining Your App](02-defining-your-app.md) — containers, functions, storage, permissions
- [Developing](03-developing.md) — `alien dev` on all platforms
