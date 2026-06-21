# Scripts

Small operational scripts used by root `package.json` commands and GitHub Actions.

## Release

- **`build-npm-packages.sh`** — Used by `.github/workflows/release.yml` to package and publish `@alienplatform/cli` from downloaded binary artifacts. Requires `VERSION`, `NODE_AUTH_TOKEN`, and the release workflow artifact layout under `./artifacts/`.

## Test environment

- **`gen-env-test.sh`** — Used by cloud test workflows and local test-infra setup to regenerate `.env.test` and `alien-manager.test.toml` from `infra/test` Terraform outputs. Requires Terraform outputs, `jq`, `AXIOM_TOKEN`, and optionally `NGROK_AUTHTOKEN` for push-mode E2E tests.
- **`select-e2e-targets.sh`** — Used by cloud test workflows after `gen-env-test.sh` to rewrite `.env.test` with canonical AWS, GCP, and Azure target aliases. Pass `--aws`, `--gcp`, and `--azure` to select non-default targets.
- **`validate-test-config.sh`** — Local validation helper documented in `infra/README.md`. It sources `.env.test`, starts `alien-manager` with `alien-manager.test.toml`, waits for `/health`, and stops the manager.

## Cloud E2E setup and cleanup

- **`configure-e2e-provider-ingress.sh`** — Used by `.github/workflows/e2e-cloud.yml` after `infra/test` Terraform apply. It configures provider-native Kubernetes baseline objects for shared EKS, GKE, and AKS clusters, including ingress classes and default storage classes.
- **`write-gke-kubeconfig.sh`** — Used by `configure-e2e-provider-ingress.sh` and GKE Terraform distribution jobs to replace Terraform's static-client kubeconfig with one authenticated as the selected target service account.
- **`cleanup-aws-e2e-resources.sh`** — Used by `.github/workflows/e2e-cloud.yml` before and after cloud E2E jobs to remove AWS resources for an E2E slot or explicit resource prefix. Run only with target-account AWS credentials and a scoped `ALIEN_E2E_SLOT` or `ALIEN_E2E_RESOURCE_PREFIX`.

## Example testing

- **`test-examples-local.sh`** (`pnpm test:examples`) — Used by CI Fast and local development to test examples against local source by temporarily injecting `pnpm.overrides`. Always restores `examples/package.json` and `examples/pnpm-lock.yaml` via trap cleanup.

## Removed legacy helpers

- **`ensure-aws-eks-oidc-provider.sh`** was removed because no tracked workflow or package command called it. EKS OIDC provider ownership now belongs to `infra/test` Terraform, and the E2E workflow imports existing bootstrap resources before apply when needed.

## Invariants

- Never commit permanent local-path dependencies into `examples/package.json`.
- `test-examples-local.sh` must always restore `examples/package.json` and `examples/pnpm-lock.yaml`.
- Shared Kubernetes bootstrap resources for EKS, GKE, and AKS are setup-owned. Runtime E2E stacks should consume them, not recreate them.
