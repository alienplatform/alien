/**
 * Run the real-addon suite under Bun with bindings present at process start.
 *
 * Bun's `process.env` mutations are not reflected in the C environment seen by
 * native addons. Production deployments already provide binding variables
 * before startup, so this parent process builds the complete fixture env and
 * launches `bun test` as a child instead of relying on a test-only addon API.
 */

import { spawnSync } from "node:child_process"
import { mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

const root = mkdtempSync(join(tmpdir(), "alien-bindings-bun-tests-"))
const env = {
  ...process.env,
  BUN_EXPECTED: "1",
  ALIEN_DEPLOYMENT_TYPE: "local",
}

function bindingEnvName(name) {
  return `ALIEN_${name.replace(/-/g, "_").toUpperCase()}_BINDING`
}

function dataDir(name) {
  const dir = join(root, name)
  mkdirSync(dir, { recursive: true })
  return dir
}

function addBindings(prefix, count, binding) {
  for (let index = 0; index < count; index += 1) {
    const name = `bun-${prefix}-${index}`
    env[bindingEnvName(name)] = JSON.stringify(binding(name))
  }
}

addBindings("kv", 10, name => ({ service: "local-kv", dataDir: dataDir(name) }))
addBindings("queue", 6, name => ({ service: "local-queue", queuePath: dataDir(name) }))
addBindings("storage", 5, name => ({ service: "local-storage", storagePath: dataDir(name) }))
addBindings("vault", 5, name => ({
  service: "local-vault",
  vaultName: "secrets",
  dataDir: dataDir(name),
}))
env[bindingEnvName("bun-process-env")] = JSON.stringify({
  service: "local-kv",
  dataDir: dataDir("bun-process-env"),
})
env[bindingEnvName("bun-container")] = JSON.stringify({
  service: "local",
  containerName: "database",
  internalUrl: "http://database.internal:5432",
  publicUrl: "http://localhost:15432",
})
const testFiles = readdirSync("tests")
  .filter(file => file.endsWith(".test.ts") && file !== "errors.test.ts")
  .map(file => `tests/${file}`)
const unitFiles = readdirSync("src/__tests__")
  .filter(file => file.endsWith(".test.ts"))
  .map(file => `src/__tests__/${file}`)

function run(args, childEnv = env) {
  const result = spawnSync("bun", ["test", ...args], { env: childEnv, stdio: "inherit" })
  return result.status ?? 1
}

try {
  let status = run([...testFiles, ...unitFiles])
  if (status === 0) {
    status = run([
      "tests/errors.test.ts",
      "--test-name-pattern",
      "bindingEnvVarName|missing binding",
    ])
  }
  if (status === 0) {
    status = run(["tests/errors.test.ts", "--test-name-pattern", "malformed binding JSON"], {
      ...env,
      [bindingEnvName("bun-bad-json")]: "not-json",
    })
  }
  if (status === 0) {
    status = run(["tests/errors.test.ts", "--test-name-pattern", "unsupported provider tag"], {
      ...env,
      [bindingEnvName("bun-redis")]: JSON.stringify({
        service: "redis",
        connectionUrl: "redis://localhost:6379",
      }),
    })
  }
  process.exitCode = status
} finally {
  rmSync(root, { recursive: true, force: true })
}
