# Admin Guide

You've received a deploy link from a software provider. This guide walks you through deploying their software to your environment.

## What's Happening

The software provider uses Alien to deploy and manage their software in your cloud. You run a one-time setup to grant access. After that, the provider pushes updates, monitors the deployment, and manages it — you don't need to do anything.

Your data stays in your cloud. The provider gets limited access to manage the deployment, not to read your data.

## Deploy to AWS / GCP / Azure (Push Model)

### 1. Install the CLI

The deploy link includes an install command. The CLI is branded for the provider — it's auto-generated:

```bash
curl -fsSL https://my-saas.alien.dev/install | bash
```

### 2. Set up cloud credentials

Make sure your cloud credentials are in the environment:

```bash
# AWS
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=us-east-1

# GCP
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/key.json

# Azure
export AZURE_CLIENT_ID=...
export AZURE_CLIENT_SECRET=...
export AZURE_TENANT_ID=...
```

### 3. Deploy

```bash
my-saas-deploy up --token dg_abc123... --platform aws
```

This creates a cross-account IAM role (AWS), service account (GCP), or managed identity (Azure) that trusts the provider's cloud account. The provider's manager uses this role to provision resources — Lambda functions, S3 buckets, etc.

The CLI returns after the initial setup. Provisioning continues in the background.

### 4. Check status

```bash
my-saas-deploy status
```

## Deploy to Kubernetes

Your provider supplies a Helm chart published to ECR Public:

```bash
helm install my-saas oci://public.ecr.aws/provider-registry/charts/my-saas \
  --set syncToken=dg_abc123... \
  --set encryptionKey=$(openssl rand -hex 32) \
  --set namespace=my-saas
```

The agent runs as a single-replica Deployment, polls the provider's manager for updates, and deploys Pods, Services, and other resources in the target namespace.

### Helm Values

| Value | Description | Default |
|-------|-------------|---------|
| `syncToken` | Token from the deploy link | (required) |
| `encryptionKey` | 64-char hex key for local state encryption | (required) |
| `namespace` | Namespace for managed resources | `""` |
| `persistence.enabled` | Persistent storage for agent state | `true` |
| `persistence.size` | PVC size | `1Gi` |
| `image.tag` | Agent image tag | `latest` |

## Deploy Locally / Single VM

```bash
my-saas-deploy up --token dg_abc123... --platform local
```

This installs an agent as a system service (systemd on Linux, launchd on macOS). The agent polls the provider's manager and deploys using Docker.

Use `--foreground` to run the agent inline (useful for testing):

```bash
my-saas-deploy up --token dg_abc123... --platform local --foreground
```

## Managing the Deployment

```bash
# Check status
my-saas-deploy status

# Stop the agent (pull model)
my-saas-deploy agent stop

# Start the agent
my-saas-deploy agent start

# Tear down the deployment
my-saas-deploy down

# Uninstall the agent service
my-saas-deploy agent uninstall
```

## What Gets Created

### AWS (Push Model)
- IAM role with cross-account trust policy
- Lambda functions
- API Gateway (for public endpoints)
- S3 buckets, DynamoDB tables (if the stack uses storage/KV)
- IAM policies scoped to the deployment

### GCP (Push Model)
- Service account with cross-project permissions
- Cloud Run services
- Load balancer (for public endpoints)
- Cloud Storage buckets, Firestore databases (if applicable)

### Kubernetes (Pull Model)
- Agent Deployment (single replica)
- Application Pods, Services, ConfigMaps
- PersistentVolumeClaim for agent state

### Local (Pull Model)
- Agent system service
- Docker containers
- Local state directory

## Security

- The provider gets a **scoped IAM role** — not your root credentials. The role is limited to the resources the deployment needs.
- **Your data stays in your cloud.** The deployment runs entirely in your environment.
- **No inbound networking required.** Push model: the provider's manager calls cloud APIs. Pull model: the agent makes outbound HTTPS requests only.
- **You can tear down at any time.** `my-saas-deploy down` removes all resources.
