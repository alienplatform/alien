# Scripts

Small operational scripts used by root `package.json` commands and GitHub Actions.

## Test environment

- **`gen-env-test.sh`** — Regenerate `.env.test` from `terraform output` after an `infra/test/` apply. See `docs/09-testing/02-test-infra-setup.md` for the full workflow.

## Example testing

- **`test-examples-published.sh`** (`pnpm test:examples`) — Test examples against published `@aliendotdev/*` packages. Falls back to local mode when packages are unavailable.
- **`test-examples-local.sh`** (`pnpm test:examples:local`) — Test examples against local source by temporarily injecting `pnpm.overrides`. Always restores `examples/package.json` via trap cleanup.

## Invariants

- Never commit permanent local-path dependencies into `examples/package.json`.
- `test-examples-local.sh` must always restore `examples/package.json`.
