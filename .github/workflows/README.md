# CI

Two tiers share one required check, `All tests passed` (`ci-fast.yml`).

| Tier | When | What runs |
| --- | --- | --- |
| Pull request | every pull request (draft or ready) and the merge queue | format, lint, typecheck, generated-file checks, `cargo check` of the whole workspace and its tests, the tests of the **changed packages**, plus the three rules below |
| Full | nightly (when `main` changed since the last green run), release qualification, manual runs, and pull requests with the `ci:full` label | every crate's tests, the whole TypeScript suite, and the rest of the heavy jobs |

Pushes to `main` do not run `ci-fast.yml`; the nightly run covers `main`.

## How the pull request tier chooses tests

`scripts/affected-rust-packages.py <base> <head> --own-only` prints what to run, and
`ci-fast.yml` reads its output. The lists it uses live in `scripts/ci-checks.json`; every entry
there has a `why` field. A change to a file outside every package (other than documentation)
is compile-checked by `Rust check` and does not drop the changed packages' tests. A
documentation-only change tests nothing heavy.

Three rules add to the changed packages' own tests:

1. **Repository-wide checks.** Tests that read files outside their own package run on every
   pull request that changes more than documentation. They need no Rust build (`repo_wide` in
   `scripts/ci-checks.json`, job `Repository-wide checks`, about 10 seconds): the install-wait
   scan over every tracked file, the release workflow checks, and the selection script's own tests.
   Run them locally with `python3 scripts/affected-rust-packages.py --run-repo-wide`.
2. **TypeScript package tests when their inputs change.** The tests of `core`, `permissions`,
   `commands` and `ai-gateway` run when a file in the package, in one of its workspace
   dependencies (read from `package.json`), or in a listed extra input changes. For `core`
   the extra inputs are `client-sdks/**` and any `openapi.json`, because its tests import the
   generated client. `bindings` is left out: its tests need the native addon, a Rust build.
3. **Dependents' output checks.** A changed package also selects named test targets of its
   direct dependents that check its output (golden, snapshot, compatibility and
   permission-rendering tests), not their whole suites. The mapping is `dependents` in
   `scripts/ci-checks.json`; today `alien-permissions` selects targets in `alien-preflights`,
   `alien-terraform`, `alien-cloudformation` and `alien-helm`. `alien-core` is not expanded
   (most packages depend on it): use the label below.

## The `ci:full` label

Adding the `ci:full` label to a pull request runs the whole workspace, like a manual run
(`RUN_ALL`). It stays on later pushes while the label is present.

Adding the label starts `ci-full-label.yml`, which calls `ci-fast.yml` on the pull request's head
commit with `full: true`. Its checks are named `<job> / …`, so they never replace the pull
request's own required `All tests passed`. `ci-fast.yml` itself does not run on `labeled` events:
a run started by an unrelated label would show a skipped `All tests passed` on the same commit
and could hide a red one.

## Adding a test that reads outside its package

If a test scans the tree or reads another package's files, add it to `repo_wide` (a command that
needs no Rust build) or, when it is a test target of a dependent, to `dependents`. The tests in
`scripts/affected-rust-packages.test.py` fail when an entry points at something that no longer exists.
