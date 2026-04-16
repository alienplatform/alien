# alien-deploy-cli

Deployment CLI for customer admins. Deploys, manages, and tears down Alien applications in target environments.

## Commands

- **`alien-deploy up`** — Deploy an application to a target environment. Supports AWS, GCP, Azure, Kubernetes, and Local platforms.
- **`alien-deploy down`** — Tear down a deployment and clean up all cloud resources.
- **`alien-deploy status`** — Show deployment status.
- **`alien-deploy list`** — List all tracked deployments.
- **`alien-deploy agent`** — Manage the alien-agent background service (install, start, stop, uninstall, status).

## How It Works

The CLI talks to the alien-manager API. On `up`, it:
1. Acquires a sync lock with the manager
2. Runs initial deployment setup using the customer's cloud credentials
3. Hands off to the manager once provisioning begins
4. Tracks the deployment locally for future `status`/`down` commands

For Kubernetes and Local platforms, `up` installs the alien-agent as a background service that syncs with the manager continuously.

## Deployment Tracking

Deployments are tracked in a local database (name → deployment ID, token, manager URL, platform), so subsequent commands work without repeating credentials.
