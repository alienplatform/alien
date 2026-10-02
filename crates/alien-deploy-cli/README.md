# alien-deploy-cli

Deployment CLI for customer admins. Deploys, manages, and tears down Alien applications in target environments.

## Commands

- **`alien-deploy deploy`** — Deploy an application to a target environment. Supports AWS, GCP, Azure, Kubernetes, and Local platforms.
- **`alien-deploy destroy`** — Tear down a deployment and clean up all cloud resources.
- **`alien-deploy status`** — Show deployment status.
- **`alien-deploy list`** — List all tracked deployments.
- **`alien-deploy agent`** — Manage the alien-operator background service (install, start, stop, uninstall, status).
- **`alien-deploy sync`** — Keep an air-gapped Kubernetes deployment up to date through one transfer folder. Online, it sends the site's reports to the manager and downloads the next signed update (only the image layers the site doesn't have). Inside the site, it checks the signature against the trusted key (`--trusted-key` on the first install, remembered after), pushes the images to the site's registry, installs or upgrades the chart, and writes a report with the deployment's state and telemetry. `--dry-run` previews; `--full` includes every layer.
- **`alien-deploy rollback`** — Return an air-gapped deployment to the release it ran before the last sync.

## How It Works

The CLI talks to the alien-manager API. On `deploy`, it:
1. Acquires a sync lock with the manager
2. Runs initial deployment setup using the customer's cloud credentials
3. Hands off to the manager once provisioning begins
4. Tracks the deployment locally for future `status`/`destroy` commands

For Kubernetes and Local platforms, `deploy` installs the alien-operator as a background service that syncs with the manager continuously. On Kubernetes, most customers install with the `helm install` command from `alien onboard` instead.

## Deployment Tracking

Deployments are tracked in a local database (name → deployment ID, token, manager URL, platform), so subsequent commands work without repeating credentials.
