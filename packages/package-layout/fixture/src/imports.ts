/**
 * Fixture import + behavior checks for the pinned package surfaces.
 *
 * This file is executed by `run.ts` under BOTH Bun (`bun src/imports.ts`) and
 * Node (`node --experimental-strip-types src/imports.ts`). It imports each of
 * the three packages' pinned surfaces from
 * `packages/{sdk,bindings,commands}/PACKAGE_LAYOUT.md` and, where the surface is
 * present, asserts the pinned error codes by their `code` field.
 *
 * Every package is loaded with its OWN dynamic `import()` inside a `try/catch`
 * so a not-yet-published package fails only its own checks — the file never dies
 * on the first missing package. Each check prints one machine-readable
 * `##CHECK##` line; `run.ts` reconciles the failing ones against
 * `expected-failures.json`. This file always exits 0: a crash with no output is
 * how `run.ts` detects an unexpected, un-reported failure.
 *
 * Today only `@alienplatform/sdk` (+ its transitive `@alienplatform/core`) is
 * installed, so the `@alienplatform/bindings` (task 04), `@alienplatform/commands`
 * (task 08), and `./worker-runtime` (task 03) checks report `fail` and `run.ts`
 * marks them `[expected]`.
 */

interface CheckLine {
  check: string
  package: string
  status: "pass" | "fail"
  reason: string
  evidence: string
}

function report(line: CheckLine): void {
  console.log(`##CHECK## ${JSON.stringify(line)}`)
}

function firstLine(value: unknown): string {
  const text = value instanceof Error ? `${value.name}: ${value.message}` : String(value)
  return text.split("\n")[0] ?? text
}

/** Names that must be present on a namespace object; returns the missing ones. */
function missingExports(mod: object, names: readonly string[]): string[] {
  return names.filter(name => !(name in mod))
}

// --- @alienplatform/sdk (facade root) — installed today, must PASS ----------
const SDK_FACADE_EXPORTS = [
  "command",
  "onStorageEvent",
  "onCronEvent",
  "onQueueMessage",
  "waitUntil",
  "storage",
  "kv",
  "queue",
  "vault",
] as const

// The sdk contract's "error re-exports" row: BindingNotConfiguredError (from
// @alienplatform/bindings) and AlienError (from @alienplatform/core). These
// arrive with task 03's facade re-export of the bindings package, so today
// they are a separate [expected] check rather than part of the main surface.
const SDK_FACADE_ERROR_REEXPORTS = ["BindingNotConfiguredError", "AlienError"] as const

async function checkSdk(): Promise<void> {
  let mod: object
  try {
    mod = await import("@alienplatform/sdk")
  } catch (err) {
    report({
      check: "import",
      package: "sdk",
      status: "fail",
      reason: "cannot import @alienplatform/sdk",
      evidence: firstLine(err),
    })
    return
  }

  const missing = missingExports(mod, SDK_FACADE_EXPORTS)
  report({
    check: "import",
    package: "sdk",
    status: missing.length === 0 ? "pass" : "fail",
    reason: missing.length === 0 ? "ok" : "facade root is missing pinned exports",
    evidence:
      missing.length === 0
        ? `resolved ${SDK_FACADE_EXPORTS.length}/${SDK_FACADE_EXPORTS.length} pinned facade exports`
        : `missing: ${missing.join(", ")}`,
  })

  const missingErrors = missingExports(mod, SDK_FACADE_ERROR_REEXPORTS)
  report({
    check: "import-error-reexports",
    package: "sdk",
    status: missingErrors.length === 0 ? "pass" : "fail",
    reason: missingErrors.length === 0 ? "ok" : "facade root is missing pinned error re-exports",
    evidence:
      missingErrors.length === 0
        ? "resolved BindingNotConfiguredError + AlienError re-exports"
        : `missing: ${missingErrors.join(", ")}`,
  })
}

// --- @alienplatform/sdk/worker-runtime — subpath OPEN until task 03 ----------
// Uses its own `check` name (distinct from the facade's "import"/"import-error-
// reexports") so per-package results stay one-assertion-per-key: run.ts's
// runtime-divergence check groups by check+package to compare Bun vs Node, and
// a shared name across unrelated assertions would let two different checks
// masquerade as "the same assertion, different runtime".
async function checkSdkWorkerRuntime(): Promise<void> {
  try {
    const mod = await import("@alienplatform/sdk/worker-runtime")
    const missing = missingExports(mod, ["runWorker"])
    if (missing.length > 0) {
      report({
        check: "import-worker-runtime",
        package: "sdk",
        status: "fail",
        reason: "worker-runtime subpath is missing pinned export runWorker",
        evidence: `missing: ${missing.join(", ")}`,
      })
      return
    }
    report({
      check: "import-worker-runtime",
      package: "sdk",
      status: "pass",
      reason: "ok",
      evidence: "resolved ./worker-runtime runWorker",
    })
  } catch (err) {
    report({
      check: "import-worker-runtime",
      package: "sdk",
      status: "fail",
      reason: "subpath ./worker-runtime is not exported",
      evidence: firstLine(err),
    })
  }
}

// --- @alienplatform/bindings — package OPEN until task 04 --------------------
// Public surface table + the shared error primitives re-export (AlienError,
// defineError from @alienplatform/core) pinned by the bindings contract.
const BINDINGS_EXPORTS = [
  "storage",
  "kv",
  "queue",
  "vault",
  "BindingNotConfiguredError",
  "AlienError",
  "defineError",
] as const

function errorCode(err: unknown): string | undefined {
  if (typeof err === "object" && err !== null && "code" in err) {
    const code = (err as { code: unknown }).code
    return typeof code === "string" ? code : undefined
  }
  return undefined
}

async function checkBindings(): Promise<void> {
  let mod: Record<string, unknown>
  try {
    mod = (await import("@alienplatform/bindings")) as Record<string, unknown>
  } catch (err) {
    report({
      check: "import",
      package: "bindings",
      status: "fail",
      reason: "cannot import @alienplatform/bindings (package not installed)",
      evidence: firstLine(err),
    })
    report({
      check: "error-code",
      package: "bindings",
      status: "fail",
      reason: "cannot assert BINDING_NOT_CONFIGURED (bindings package not installed)",
      evidence: "bindings import failed; see the import check above",
    })
    return
  }

  const missing = missingExports(mod, BINDINGS_EXPORTS)
  report({
    check: "import",
    package: "bindings",
    status: missing.length === 0 ? "pass" : "fail",
    reason: missing.length === 0 ? "ok" : "missing pinned exports",
    evidence: missing.length === 0 ? "resolved storage/kv/queue/vault + error" : missing.join(", "),
  })

  // The first operation against an unconfigured binding must throw
  // BINDING_NOT_CONFIGURED naming ALIEN_<NAME>_BINDING (bindings behavior
  // contract). No deployment env is supplied: with no deployment type and no
  // credentials, construction still succeeds and the missing-binding error must
  // surface before any platform resolution. `get` is the pinned first storage
  // operation.
  try {
    const storageFactory = mod.storage as (name: string) => Record<string, unknown>
    const handle = storageFactory("layout-fixture-probe")
    const firstOp = handle.get as ((key: string) => Promise<unknown>) | undefined
    if (typeof firstOp !== "function") {
      throw new Error("no first operation available to trigger BINDING_NOT_CONFIGURED yet")
    }
    await firstOp("probe")
    report({
      check: "error-code",
      package: "bindings",
      status: "fail",
      reason: "expected BINDING_NOT_CONFIGURED but no error was thrown",
      evidence: "unconfigured storage operation did not throw",
    })
  } catch (err) {
    const code = errorCode(err)
    report({
      check: "error-code",
      package: "bindings",
      status: code === "BINDING_NOT_CONFIGURED" ? "pass" : "fail",
      reason:
        code === "BINDING_NOT_CONFIGURED" ? "ok" : "expected error code BINDING_NOT_CONFIGURED",
      evidence: `code=${code ?? "<none>"}: ${firstLine(err)}`,
    })
  }
}

// --- @alienplatform/commands — package OPEN until task 08 -------------------
const COMMANDS_EXPORTS = [
  "CommandsClient",
  "createCommandReceiver",
  "CommandReceiverConfigInvalidError",
  // Shared error primitives re-export (AlienError, defineError from
  // @alienplatform/core) pinned by the commands contract.
  "AlienError",
  "defineError",
] as const

async function checkCommands(): Promise<void> {
  let mod: Record<string, unknown>
  try {
    mod = (await import("@alienplatform/commands")) as Record<string, unknown>
  } catch (err) {
    report({
      check: "import",
      package: "commands",
      status: "fail",
      reason: "cannot import @alienplatform/commands (package not installed)",
      evidence: firstLine(err),
    })
    report({
      check: "error-code",
      package: "commands",
      status: "fail",
      reason: "cannot assert COMMAND_RECEIVER_CONFIG_INVALID (commands package not installed)",
      evidence: "commands import failed; see the import check above",
    })
    return
  }

  const missing = missingExports(mod, COMMANDS_EXPORTS)
  report({
    check: "import",
    package: "commands",
    status: missing.length === 0 ? "pass" : "fail",
    reason: missing.length === 0 ? "ok" : "missing pinned exports",
    evidence:
      missing.length === 0
        ? "resolved CommandsClient/createCommandReceiver + error"
        : missing.join(", "),
  })

  // An empty/invalid receiver environment must throw
  // COMMAND_RECEIVER_CONFIG_INVALID naming ALIEN_COMMANDS_URL (commands behavior
  // contract). Force an empty value so the call must fail regardless of ambient env.
  process.env.ALIEN_COMMANDS_URL = ""
  try {
    const createReceiver = mod.createCommandReceiver as () => unknown
    createReceiver()
    report({
      check: "error-code",
      package: "commands",
      status: "fail",
      reason: "expected COMMAND_RECEIVER_CONFIG_INVALID but no error was thrown",
      evidence: "createCommandReceiver() with empty env did not throw",
    })
  } catch (err) {
    const code = errorCode(err)
    report({
      check: "error-code",
      package: "commands",
      status: code === "COMMAND_RECEIVER_CONFIG_INVALID" ? "pass" : "fail",
      reason:
        code === "COMMAND_RECEIVER_CONFIG_INVALID"
          ? "ok"
          : "expected error code COMMAND_RECEIVER_CONFIG_INVALID",
      evidence: `code=${code ?? "<none>"}: ${firstLine(err)}`,
    })
  }
}

await checkSdk()
await checkSdkWorkerRuntime()
await checkBindings()
await checkCommands()
