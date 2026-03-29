# Onboarding Customers

## Create a Deployment Group

A *deployment group* represents one customer or environment:

```bash
alien onboard acme-corp
```

```
Deployment group 'acme-corp' created.

Deploy link:
  https://my-saas.alien.dev/deploy#token=dg_abc123...
```

Send the deploy link to the customer's admin.

## What the Admin Sees

The admin clicks the link and lands on a branded deploy page — your logo, your name, your colors. They pick their platform and follow the instructions.

Alien auto-generates a branded CLI for your project. Instead of installing "alien", the admin installs *your* CLI:

```bash
curl -fsSL https://manager.alien.dev/install | bash
```

### One-Time Setup

The admin runs the deploy command. This is the only time they're involved:

```bash
my-saas-deploy up --token dg_abc123... --platform aws
```

This grants limited cross-account access (an IAM role that trusts *your* AWS account, not Alien's) and provisions frozen infrastructure — storage, networking, IAM policies — with elevated permissions. The admin's involvement ends here.

### Ongoing Updates

From now on, every `alien release` automatically updates every customer's deployment. Only live resources (functions, containers) are updated, with minimal permissions. No admin involvement, no version drift.

## Push vs Pull

Two deployment models, depending on the customer's environment:

**Push** — the manager calls cloud APIs directly using the cross-account role the admin created. Best for AWS, GCP, Azure where you want full control over provisioning.

**Pull** — a lightweight agent runs in the customer's environment, polls for updates every 30 seconds, and deploys locally. Works on every platform. No cross-account access needed — the agent uses local credentials. Best for Kubernetes, on-prem, firewalled environments.

The admin chooses the model when they run `my-saas-deploy up --platform <platform>`.

## Kubernetes

For Kubernetes customers, a Helm chart published to ECR Public:

```bash
helm install my-saas oci://public.ecr.aws/your-registry/charts/my-saas \
  --set syncToken=dg_abc123... \
  --set encryptionKey=$(openssl rand -hex 32) \
  --set namespace=my-saas
```

The agent runs as a single-replica Deployment, polls for updates, and manages Pods, Services, and other resources in the target namespace.

## Per-Customer Configuration

Set environment variables per deployment group:

```bash
alien onboard acme-corp \
  --env CUSTOMER_ID=acme \
  --env FEATURE_TIER=enterprise
```

Or update later:

```bash
alien deployments set-env <deployment-id> \
  --env FEATURE_TIER=enterprise-plus
```

These merge with variables defined in `alien.ts`. Customer-specific values override stack defaults.

## Monitoring

```bash
alien deployments ls
```

```
NAME              STATUS    PLATFORM  RELEASE     UPDATED
acme-corp         running   aws       rel_abc123  2 min ago
beta-customer     running   gcp       rel_abc123  5 min ago
local-dev         running   local     rel_abc123  just now
```

View logs and traces in the dashboard, or forward telemetry to your own observability stack via OTLP.

## Next

- [Cloud-Agnostic Bindings](05-bindings.md) — storage, KV, queues from your app code
- [Remote Commands](06-commands.md) — invoke code without inbound networking
- [Admin Guide](10-admin-guide.md) — the full guide for your customer's admin
