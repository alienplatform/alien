/**
 * Fixture builder for the local-provider binding env vars, matching EXACTLY
 * the JSON shapes `alien-local` (and the Rust `alien-core::bindings` types)
 * write for `ALIEN_<NAME>_BINDING`:
 *
 * - Storage: `crates/alien-core/src/bindings/storage.rs::LocalStorageBinding`
 *   → `{"service":"local-storage","storagePath":"<dir>"}`
 * - Kv: `crates/alien-core/src/bindings/kv.rs::LocalKvBinding`
 *   → `{"service":"local-kv","dataDir":"<dir>","keyPrefix"?:"<prefix>"}`
 * - Queue: `crates/alien-core/src/bindings/queue.rs::LocalQueueBinding`
 *   → `{"service":"local-queue","queuePath":"<dir>"}` — a *directory*; the
 *     `localqueue.v1` sqlite file (`localqueue.sqlite`) is created inside it
 *     (see the doc comment on `LocalQueueBinding::queue_path` and
 *     `crates/alien-bindings/src/providers/local_store.rs::LocalStore::open`).
 * - Vault: `crates/alien-core/src/bindings/vault.rs::LocalVaultBinding`
 *   → `{"service":"local-vault","vaultName":"<name>","dataDir":"<dir>"}` —
 *     secrets live at `<dir>/secrets.json`
 *     (see `crates/alien-bindings/src/providers/vault/local.rs`).
 *
 * Cross-checked against `crates/alien-local/src/local_bindings_provider.rs`
 * (which extracts exactly these fields from the parsed binding) and against
 * `crates/alien-bindings/src/bindings.rs`'s own `#[cfg(test)]` fixtures, which
 * build the identical JSON by hand.
 *
 * Every builder also folds in `ALIEN_DEPLOYMENT_TYPE=local`: resolving an
 * already-*configured* binding (as opposed to reporting it missing) runs the
 * full `BindingsProvider::from_env` path, which resolves the deployment
 * platform eagerly (see `crates/alien-bindings/src/provider.rs`
 * `LazyEnvBindingsProvider::provider`). Without it every real operation fails
 * with `ENVIRONMENT_VARIABLE_MISSING` before it ever reaches the binding.
 */

import { mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

/** The env var every binding operation needs in addition to its own JSON. */
export const LOCAL_DEPLOYMENT_ENV: Record<string, string> = {
  ALIEN_DEPLOYMENT_TYPE: "local",
}

/**
 * Derive the `ALIEN_<NAME>_BINDING` env var name for `bindingName`, mirroring
 * `alien_core::bindings::binding_env_var_name` (uppercase, `-` → `_`).
 *
 * `bindingEnvVarName("my-files") === "ALIEN_MY_FILES_BINDING"`.
 */
export function bindingEnvVarName(bindingName: string): string {
  return `ALIEN_${bindingName.replace(/-/g, "_").toUpperCase()}_BINDING`
}

/**
 * Every directory handed out by `makeTempDir` since the last `cleanupTempDirs`
 * call, so a test file can remove exactly what it created without guessing at
 * paths. A test file's own `afterAll(cleanupTempDirs)` clears this before the
 * next file's tests run, so it's safe even if the runner shares this module
 * instance across files (Bun's single-process test runner does; Vitest's
 * per-file isolation doesn't, but the same call is a harmless no-op there).
 */
const createdDirs = new Set<string>()
const createdEnvKeys = new Set<string>()

/** Install a fixture in the process environment used by the public factories. */
export function installBindingEnv(env: Record<string, string>): void {
  for (const [key, value] of Object.entries(env)) {
    process.env[key] = value
    if (key !== "ALIEN_DEPLOYMENT_TYPE") createdEnvKeys.add(key)
  }
}

/** Create a fresh, empty temp directory for one binding's on-disk state. */
export function makeTempDir(label: string): string {
  const dir = mkdtempSync(join(tmpdir(), `alien-bindings-test-${label}-`))
  createdDirs.add(dir)
  return dir
}

/**
 * Remove every temp directory created via `makeTempDir` so far and forget
 * them. Call from an `afterAll` in any test file that builds fixtures through
 * this helper, so `os.tmpdir()` doesn't accumulate one directory per test run.
 */
export function cleanupTempDirs(): void {
  for (const dir of createdDirs) {
    rmSync(dir, { recursive: true, force: true })
  }
  createdDirs.clear()
  for (const key of createdEnvKeys) delete process.env[key]
  createdEnvKeys.clear()
}

/** An env map plus the temp directory backing it, for tests that want to inspect disk state. */
export interface LocalBindingFixture {
  env: Record<string, string>
  dir: string
}

/** Build the env for a `local-storage` binding rooted at a fresh temp dir. */
export function localStorageBindingEnv(
  bindingName: string,
  dir = makeTempDir(`storage-${bindingName}`),
): LocalBindingFixture {
  const fixture = {
    dir,
    env: {
      ...LOCAL_DEPLOYMENT_ENV,
      [bindingEnvVarName(bindingName)]: JSON.stringify({
        service: "local-storage",
        storagePath: dir,
      }),
    },
  }
  installBindingEnv(fixture.env)
  return fixture
}

/** Build the env for a `local-kv` binding rooted at a fresh temp dir. */
export function localKvBindingEnv(
  bindingName: string,
  options: { dir?: string; keyPrefix?: string } = {},
): LocalBindingFixture {
  const dir = options.dir ?? makeTempDir(`kv-${bindingName}`)
  const binding: Record<string, unknown> = { service: "local-kv", dataDir: dir }
  if (options.keyPrefix !== undefined) binding.keyPrefix = options.keyPrefix

  const fixture = {
    dir,
    env: {
      ...LOCAL_DEPLOYMENT_ENV,
      [bindingEnvVarName(bindingName)]: JSON.stringify(binding),
    },
  }
  installBindingEnv(fixture.env)
  return fixture
}

/** Build the env for a `local-queue` binding rooted at a fresh temp dir. */
export function localQueueBindingEnv(
  bindingName: string,
  dir = makeTempDir(`queue-${bindingName}`),
): LocalBindingFixture {
  const fixture = {
    dir,
    env: {
      ...LOCAL_DEPLOYMENT_ENV,
      [bindingEnvVarName(bindingName)]: JSON.stringify({
        service: "local-queue",
        queuePath: dir,
      }),
    },
  }
  installBindingEnv(fixture.env)
  return fixture
}

/** Build the env for a `local-vault` binding rooted at a fresh temp dir. */
export function localVaultBindingEnv(
  bindingName: string,
  vaultName: string,
  dir = makeTempDir(`vault-${bindingName}`),
): LocalBindingFixture {
  const fixture = {
    dir,
    env: {
      ...LOCAL_DEPLOYMENT_ENV,
      [bindingEnvVarName(bindingName)]: JSON.stringify({
        service: "local-vault",
        vaultName,
        dataDir: dir,
      }),
    },
  }
  installBindingEnv(fixture.env)
  return fixture
}

/** Build the env for a local linked-container binding. */
export function localContainerBindingEnv(bindingName: string): void {
  installBindingEnv({
    ...LOCAL_DEPLOYMENT_ENV,
    [bindingEnvVarName(bindingName)]: JSON.stringify({
      service: "local",
      containerName: "database",
      internalUrl: "http://database.internal:5432",
      publicUrl: "http://localhost:15432",
    }),
  })
}

/** Return the sole element of `items`, throwing (with a useful message) if there isn't exactly one. */
export function only<T>(items: T[]): T {
  if (items.length !== 1) {
    throw new Error(`expected exactly one item, got ${items.length}: ${JSON.stringify(items)}`)
  }
  const [item] = items
  if (item === undefined) throw new Error("unreachable: length check above guarantees an element")
  return item
}
