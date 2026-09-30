import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import test from "node:test"
import { fileURLToPath } from "node:url"

const root = fileURLToPath(new URL("../..", import.meta.url))
const config = join(root, "tools/oxlint/anti-slop.test.json")

function antiSlopDiagnostics(source) {
  const directory = mkdtempSync(join(tmpdir(), "alien-anti-slop-"))
  const fixture = join(directory, "fixture.ts")
  try {
    writeFileSync(fixture, source)
    const result = spawnSync(
      "pnpm",
      ["exec", "oxlint", "--config", config, "--format", "json", fixture],
      { cwd: root, encoding: "utf8" },
    )
    assert.equal(result.error, undefined)
    assert.ok(result.status === 0 || result.status === 1, result.stderr)
    const output = JSON.parse(result.stdout)
    assert.equal(output.number_of_files, 1)
    return output.diagnostics
      .map(diagnostic => diagnostic.code)
      .filter(code => code.startsWith("anti-slop("))
      .sort()
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
}

test("vendored anti-slop rules report their intended patterns", () => {
  const diagnostics = antiSlopDiagnostics(`
    export const filterMap = [1, 2].filter(Boolean).map(String)
    export const copied = [1, 2].reduce((acc, value) => {
      const alias = acc
      return alias.concat(value)
    }, [] as number[])
    export const copiedArray = [1, 2].reduce((acc) => Array.from(acc), [] as number[])
    export const copiedObject = [1, 2].reduce((acc) => Object.assign({}, acc), {})
    export const conditional = { ...(true ? { enabled: true } : {}) }
    export const reflectedGet = Reflect.get({}, "value")
    export const reflectedApply = Reflect.apply(() => 1, null, [])
    export const asserted = ({} as unknown) as { value: string }
  `)

  assert.deepEqual(diagnostics, [
    "anti-slop(no-array-filter-map)",
    "anti-slop(no-chained-type-assertions)",
    "anti-slop(no-conditional-empty-object-spread)",
    "anti-slop(no-reduce-accumulator-copy)",
    "anti-slop(no-reduce-accumulator-copy)",
    "anti-slop(no-reduce-accumulator-copy)",
    "anti-slop(no-reflect-apply)",
    "anti-slop(no-reflect-get)",
  ])
})

test("vendored anti-slop rules leave safe patterns and shadowed globals alone", () => {
  const diagnostics = antiSlopDiagnostics(`
    const Reflect = { get: () => 1, apply: () => 1 }
    const Array = { from: (values: number[]) => values }
    const Object = { assign: (...parts: object[]) => parts[0] }
    export const shadowedGet = Reflect.get()
    export const shadowedApply = Reflect.apply()
    export const shadowedArray = [1, 2].reduce((acc) => Array.from(acc), [] as number[])
    export const shadowedObject = [1, 2].reduce((acc) => Object.assign({}, acc), {})
    export const flatMapped = [1, 2].flatMap(value => value ? [value] : [])
    export const spread = { ...(true ? { enabled: true } : { disabled: true }) }
    export const asserted = {} as { value?: string }
  `)

  assert.deepEqual(diagnostics, [])
})
