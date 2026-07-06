# `@alienplatform/package-layout`

Executable fixture that proves the packed publishable packages install and import
correctly on Bun and Node, honoring the surfaces pinned in
[`packages/sdk`](../sdk/PACKAGE_LAYOUT.md),
[`packages/bindings`](../bindings/PACKAGE_LAYOUT.md), and
[`packages/commands`](../commands/PACKAGE_LAYOUT.md).

It is not a unit test. It packs the real tarballs (`pnpm pack`), installs them into
a throwaway consumer with **npm** (a real downstream consumer, not the pnpm
workspace), and then imports, typechecks, and — for the binding native embed —
`bun build --compile`s them. This catches packaging bugs (`exports` maps, shipped
declarations, `file:`/override resolution, tarball contents) that never show up
when everything is a workspace symlink.

## Run it

```sh
# From the workspace (the invocation the ticket specifies):
pnpm --filter @alienplatform/package-layout run check

# Bun-first, from packages/package-layout:
bun run.ts

# Node (what the `check` script runs under):
node --experimental-strip-types run.ts
```

The orchestrator (`run.ts`) runs under either Node or Bun. Whichever drives it, it
then exercises the installed consumer under **both** runtimes: `bun src/imports.ts`
and `node --experimental-strip-types src/imports.ts`. The Node import check
deliberately runs `node --experimental-strip-types` against the **raw `.ts`
fixture source** (no build step) — the same runner decision as the validator in
`packages/scripts`; the shipped-declaration surface is covered separately by the
`typecheck` step. If `bun` is not on `PATH`, the Bun-only steps print
`[env-skip]` and are dropped from reconciliation (CI provides Bun via
`setup-bun`).

Requires: Node ≥ 22 (native `--experimental-strip-types`), Bun ≥ 1.0, and network
access to the npm registry for the non-`@alienplatform` transitive dependencies.
Every transitive `@alienplatform/*` package is pinned to a packed tarball via
`overrides`, so those never come from the registry.

## Expected-failure semantics

Most of the pinned surface is not implemented yet: `@alienplatform/bindings`'s
per-platform prebuilds (task 04a), `@alienplatform/commands` (task 08), and the
sdk `./worker-runtime` subpath (task 03). The fixture is **designed to run
today** and fail only on those not-yet-landed pieces.

- Each discrete check reports `PASS`, `[expected]`, or `FAIL` with an evidence
  line.
- Every failure that is expected right now is listed in
  [`expected-failures.json`](./expected-failures.json) with the owning task, using
  the same `{ check, package, reason, owningTask }` shape as the static validator
  in [`packages/scripts`](../scripts/expected-failures.json).
- The run exits `0` **only** when the actual failure set matches that list exactly
  — zero unexpected failures **and** zero stale expectations (an expectation that
  no longer occurs fails the run too, so the list cannot silently rot). This reuses
  the validator's `applyExpectedFailures` + `exitCodeFor`.
- When a task lands its package, its checks flip from `[expected]` to `PASS` and
  the owning task **must delete** the now-stale entries from `expected-failures.json`
  (the run will fail loudly until it does).

## What each step proves

| Step | Proves |
|---|---|
| `pack` | Each publishable package (`sdk`, `core`, `bindings`) produces a tarball; `commands` is `[expected]` absent (08). |
| `write-manifest` | The consumer manifest is rewritten to `file:` the packed tarballs with `overrides` pinning every transitive `@alienplatform/*`. |
| `install` / `install-resolution` | `npm install` succeeds and `@alienplatform/{sdk,core}` resolve to the **tarballs**, not the registry. |
| `import` (bun + node) | The pinned surfaces import under both runtimes, including each contract's pinned error re-exports (`AlienError`/`defineError`; `BindingNotConfiguredError`; `CommandReceiverConfigInvalidError`). The sdk facade's error re-exports (03), `./worker-runtime` (03), and `@alienplatform/commands` (08) are `[expected]` missing. |
| `error-code` (bun + node) | `BINDING_NOT_CONFIGURED` / `COMMAND_RECEIVER_CONFIG_INVALID` are asserted by their `code` field once the packages exist; `[expected]` until 08 for commands. |
| `typecheck` | The consumer typechecks under NodeNext/strict against the shipped declarations; only the not-yet-landed module specifiers are `[expected]` unresolved. |
| `packed-contents` | Each tarball's file list matches an **exact** expected set — see the note below. Required artifacts (`dist/*.js`, `package.json`, `PACKAGE_LAYOUT.md` for the contract packages) must be present, and any file outside the set fails the run. The per-platform prebuild's exact `.node`+manifest shape is `[expected]` until 04a. |
| `compile` | `bun build --compile` of the pinned `./native` embed entry produces a runnable single-file binary; `[expected]` to fail until 04a stages a per-platform `.node` prebuild. |
| `validator` | The static layout validator in `packages/scripts` still passes. |

### Note on the `packed-contents` expected set

The expected file set per package is: `package.json`, `README*`, `LICENSE*`,
`PACKAGE_LAYOUT.md` (when the package owns one), and `dist/**`. When a manifest
carries a `files` allowlist, the set is derived from that field instead (plus the
files npm always includes). Because no publishable manifest carries `files` today,
sdk/core also ship a handful of source/config files; those are **explicitly
enumerated** in `EXTRA_SHIPPED_TODAY` in `run.ts` (no wildcard allowance) with the
tightening owned by the package-restructure tasks (03/17). Any file outside the
union fails the run — adding an unexpected file to a published tarball is a
contract violation this fixture catches.

## Files

- `run.ts` — orchestrator (pack → rewrite manifest → npm install → import/typecheck/
  contents/compile → validator), reconciled against `expected-failures.json`.
- `expected-failures.json` — the task-owned failures allowed today.
- `fixture/package.json` — the npm consumer. `run.ts` rewrites `dependencies` and
  `overrides` on every run to point at the freshly packed tarballs; every other
  field is committed and preserved.
- `fixture/tsconfig.json` — self-contained NodeNext/strict config (the consumer is
  installed by npm, so it cannot extend the workspace tsconfig).
- `fixture/src/imports.ts` — per-package dynamic imports + error-code assertions.
- `fixture/src/compile-entry.ts` — static `./native` import for `bun build --compile`.

`.tarballs/`, `fixture/node_modules/`, `fixture/package-lock.json`, and
`fixture/.compiled/` are generated per run and git-ignored.
