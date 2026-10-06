# alien-deploy-cli

Deployment CLI for customer admins. Deploys, manages, and tears down Alien applications in target environments.

## Commands

- **`alien-deploy deploy`** — Deploy an application to a target environment. Supports AWS, GCP, Azure, Kubernetes, Machines, and Local platforms.
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

## Machines setup updates from automation

When an update changes Frozen resources, setup must run before the manager can continue. A fresh runner can target the existing deployment without a local tracking entry:

```sh
./democtl deploy --setup-update \
  --deployment-id "$DEPLOYMENT_ID" \
  --update-operation-id "$UPDATE_OPERATION_ID" \
  --release-id "$RELEASE_ID" \
  --platform machines \
  --token-file /run/secrets/setup-token \
  --base-url "$PLATFORM_API_URL" \
  --config deployment.toml
```

Use the generated installer's command name in place of `democtl`. Supply the exact blocked update operation and release from the authorized setup session, and a deployment-group setup token. A runtime deployment token does not authorize setup preparation.

For externally owned S3-compatible storage, `deployment.toml` can contain:

```toml
platform = "machines"

[externalBindings.archive]
type = "storage"
service = "s3"
bucketName = "customer-archive"
endpoint = "https://storage.example.com"
region = "us-east-1"
forcePathStyle = true
```

`archive` must identify a Storage resource in the target release. Machines bindings contain only the store locator: omit `accessKeyId` and `secretAccessKey`. The workload uses ambient AWS credentials. For static credentials, declare Secret stack inputs mapped to `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` and supply them through the supported encrypted input or secret-store delivery path. A Kubernetes `secretRef` is not a Machines credential source. Ordinary external bindings do not require `remoteAccess: true`, and setup does not create or delete their underlying storage.

For the same binding type and service, omitted fields keep their saved values; explicit fields are patched. Changing the type or service replaces the binding entirely. The TOML configuration has no null-valued field-clearing syntax.

The command preserves unrelated target settings, saves explicit choices, follows the returned operation ID, and verifies the acquired target before setup runs. It never initializes a new deployment. If setup is interrupted after saving, resume with the operation ID printed by the command; the old operation ID is deliberately refused. Explicit non-secret `--input` choices and `[inputs]` values are saved through the deployment input API with the same operation check before bindings are prepared. Deployer secret values are refused through `--input`, `--secret-input`, and `[secretInputs]`; write them directly into the deployment's configured secret store. Setup reports missing secrets until they are available. Existing gate answers remain fixed by setup policy, and omitted inputs retain their stored values.

A successful setup command means the manager may continue provisioning. Verify the exact release reaches Running and exercise storage read/write from the workload before treating the update as complete.
