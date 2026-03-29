# Private Manager

By default, Alien runs the manager for you (an alien-hosted manager). If you need your code and data to stay in your own cloud, you can deploy a private manager:

```bash
alien manager deploy --platform aws --region us-east-1
```

Alien provisions the manager and all its dependencies (container registry, storage, IAM roles) in your AWS account and keeps it running and updated — the same way it manages any other deployment.

## Why a Private Manager?

In the alien-hosted tier, your container images and command payloads pass through Alien's infrastructure. Some developers need stricter isolation:

- **Compliance** — regulated industries require the control plane in the developer's own account
- **Data residency** — command payloads may contain customer data that can't leave a specific region
- **Trust boundary** — source code and customer data stay on your infrastructure only

## What Gets Created

All in your cloud account:

- **Container registry** (ECR / GAR / ACR) — deployment images
- **Object storage** (S3 / GCS / Blob) — command payloads and telemetry
- **KV store** (DynamoDB / Firestore / Table Storage) — command state
- **Cross-account role** — for push-model deployments into customer accounts
- **The manager container** — runs the deployment loop

Preview before deploying:

```bash
alien manager deploy --platform aws --region us-east-1 --dry-run
```

## Same Features, Different Data Residency

Everything works the same — CLI, dashboard, releases, commands, telemetry. The only difference is where data lives:

| Data | Alien-Hosted | Private Manager |
|------|---------|-------------|
| Container images | Alien's registry | Your registry |
| Command payloads | Alien's storage | Your storage |
| Telemetry | Alien's DeepStore | Your storage |

Alien sees deployment metadata (status, timing, resource names). Alien never sees your source code, container images, command payloads, or customer data.

## Managing the Manager

The manager is an Alien deployment — it auto-updates, auto-heals, and scales. View its status in the dashboard or:

```bash
alien manager status
alien manager logs
```

## Next

- [Standalone Mode](09-standalone.md) — run without an alien.dev account at all
