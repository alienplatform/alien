# Scripts

Small operational scripts used by root `package.json` commands and GitHub Actions.

## Release

- `python3 scripts/test-cli-upgrade.py /path/to/stable /path/to/canary /path/to/next-canary --sdk-version 3.3.30` exercises the CLI updater through a local HTTP server and disposable executable copies. Set `--sdk-version` to the `workspace.package.version` from the `Cargo.toml` used to build both canaries. Build the canary fixtures with `ALIEN_CLI_VERSION=3.3.31-ffffffff` and `ALIEN_CLI_VERSION=3.3.31-00000001`, using a base version newer than the stable fixture. It checks channel switching, checksum and version validation, dry runs, force, and package-manager refusal.

- **`smoke-sandbox-agent.sh`** — Used by `.github/workflows/release.yml` to qualify the published `alien-sandbox-agent` image on both `linux/amd64` and `linux/arm64`, and usable on any GCP sandbox image built from the same contract. Takes the image reference and needs a Docker daemon that can pull it. `--platforms` narrows the platforms probed, and `--tools docker/sandbox-default-tools.txt` also runs every tool of the default sandbox base as the image's user. `scripts/smoke-sandbox-agent.test.sh` runs every probe and every failure mode against a stub `docker`, so the script itself is testable without a daemon or an image.

## Test environment

- **`gen-env-test.sh`** — Used by cloud test workflows and local test-infra setup to regenerate `.env.test` and `alien-manager.test.toml` from `infra/test` Terraform outputs. Requires Terraform outputs, `jq`, `AXIOM_TOKEN`, and optionally `NGROK_AUTHTOKEN` for push-mode E2E tests.
- **`select-e2e-targets.sh`** — Used by cloud test workflows after `gen-env-test.sh` to rewrite `.env.test` with canonical AWS, GCP, and Azure target aliases. Pass `--aws`, `--gcp`, and `--azure` to select non-default targets.
- **`validate-test-config.sh`** — Local validation helper documented in `infra/README.md`. It sources `.env.test`, starts `alien-manager` with `alien-manager.test.toml`, waits for `/health`, and stops the manager.

## AWS permissions boundary validation

`python3 scripts/validate-aws-boundary.py --account ACCOUNT --region us-west-2` compiles the automatic management profile and runtime boundary from source, validates a CloudFormation template containing the boundary, and exercises role writes, adoption, and workload access across two prefixes on AWS. Requires boto3, Cargo, and administrative credentials for a disposable test account. The script verifies the account before creating randomly named resources and removes its roles, policies, buckets, function, and logs even when a check fails. Authorization checks reject unrelated API errors. Account-wide EC2 discovery is expected to remain allowed by the catalog’s volume-backup runtime grants. This tests the generated policies; it does not validate setup-package activation or handoff.

## Cloud E2E setup and cleanup

- **`configure-e2e-provider-ingress.sh`** — Used by `.github/workflows/e2e-cloud.yml` after `infra/test` Terraform apply. It configures provider-native Kubernetes baseline objects for shared EKS, GKE, and AKS clusters, including ingress classes and default storage classes.
- **`write-gke-kubeconfig.sh`** — Used by `configure-e2e-provider-ingress.sh` and GKE Terraform distribution jobs to replace Terraform's static-client kubeconfig with one authenticated as the selected target service account.
- **`cleanup-aws-e2e-resources.sh`** — Used by `.github/workflows/e2e-cloud.yml` before and after cloud E2E jobs to remove AWS resources for an E2E slot or explicit resource prefix. Run only with target-account AWS credentials and a scoped `ALIEN_E2E_SLOT` or `ALIEN_E2E_RESOURCE_PREFIX`.

## Sandbox egress guard

- **`egress-deny-guard-teardown.sh`** — Used by `.github/workflows/egress-deny-guard.yml` before and after the weekly `egressDeny` check to remove the probe stack, and to fail the run if any of it survives. Takes the stack name and needs target-account AWS credentials in both the CLI's and the Rust client's environment (`AWS_TARGET_*`).

  Two preconditions live outside this repository: the OIDC role needs `MaxSessionDuration` of at least 5400 seconds, or every run dies at credential assumption; and a lifecycle rule expiring the artifact bucket's `egress-deny-guard/` prefix is what reclaims a bundle a cancelled run could not delete.

## Example testing

- **`test-examples-local.sh`** (`pnpm test:examples`) — Used by CI Fast and local development to test examples against local source by temporarily injecting `pnpm.overrides`. Always restores `examples/package.json` and `examples/pnpm-lock.yaml` via trap cleanup.

## Signed releases

- **`signed-release-e2e/run.py`** — End-to-end attack suite for protected mode. It runs a real `alien-manager`, a real Operator in a kind cluster and a real signed `alien release`. Signing is set up as a team sets it up: a TPM simulator (`swtpm`) holds the developer's device key, the first `alien release` in a terminal turns signing on, `alien devices add-ci` (with a stand-in `gh`) creates the CI key, and CI signs every later release through the real CLI. Protected mode installs the signed chart the release carries; unprotected mode installs the ordinary chart. A proxy between the Operator and the manager plays a compromised control plane, one attack at a time: a forged signer list, an unsigned or edited release, a changed stack, an older release, manager-set environment variables, a redirected deployer input, an Operator image no release names, a dynamic container, another image behind the release's reference, an unsigned release, and (protected only) a forged, misdirected and replayed signed config. `--mode protected` expects each attack to be refused with nothing in the namespace changed. `--mode unprotected` expects each attack to take effect, which shows the attacks are real. Both modes check that a manager-changed developer input is applied only without protection, that a normal release rolls out, and that a rollback by re-release works. In protected mode the suite ends by installing the ordinary chart for a second customer, letting the manager start a dynamic container, then upgrading to the signed chart and expecting the container to be removed (`--only protection-turned-on` runs just that step). Needs docker, kind, kubectl, helm, swtpm, tpm2-tools, `script` and debug builds of `alien-manager` and `alien-operator` (from `target/debug`, or `--binaries <dir>`). The CLI must carry the Operator pin of the image the suite publishes, so the suite builds it itself (`cargo build -p alien-cli --features test-device-env`, with the pin in two tracked files for the build only), or takes one built that way with `--alien`. `--down` removes what it created.
- **`signed-release-e2e/airgapped.py`** — The same attacks for an air-gapped site. `alien-deploy sync` runs as two machines: online through the proxy, which gets each bundle signed by the manager, and at the site against its own registry and cluster. Attacks: an unsigned target, a changed command, an older release, a manager-set `LD_PRELOAD`, a chart swapped in the registry that ignores `releaseSigning`, and an `alien-deploy` build the manager adds to the bundle. It also checks that `alien-deploy rollback` is refused, that the first protected install needs `--accept-chart`, and that normal and re-released versions install with no admin action. The app is released for `linux-x64` and `linux-arm64`, so its signed image is a multi-platform index, and the site declares one architecture: the suite checks that the site runs the signed index digest. Same modes as `run.py`, plus a debug build of `alien-deploy`.

## Removed legacy helpers

- **`ensure-aws-eks-oidc-provider.sh`** was removed because no tracked workflow or package command called it. EKS OIDC provider ownership now belongs to `infra/test` Terraform, and the E2E workflow imports existing bootstrap resources before apply when needed.

## Invariants

- Never commit permanent local-path dependencies into `examples/package.json`.
- `test-examples-local.sh` must always restore `examples/package.json` and `examples/pnpm-lock.yaml`.
- Shared Kubernetes bootstrap resources for EKS, GKE, and AKS are setup-owned. Runtime E2E stacks should consume them, not recreate them.
