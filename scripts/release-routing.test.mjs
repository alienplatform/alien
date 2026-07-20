import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import test from "node:test"

const workflow = readFileSync(resolve(process.cwd(), ".github/workflows/release.yml"), "utf8")

function parseJobs(source) {
  const jobs = new Map()
  const lines = source.split("\n")
  const jobsIndex = lines.findIndex(line => line === "jobs:")
  assert.notEqual(jobsIndex, -1, "release workflow has jobs")

  let current
  for (const line of lines.slice(jobsIndex + 1)) {
    const job = /^  ([a-z0-9_-]+):$/.exec(line)
    if (job) {
      current = { if: "", uses: "" }
      jobs.set(job[1], current)
      continue
    }
    if (!current) continue
    const condition = /^    if: (.+)$/.exec(line)
    if (condition) current.if = condition[1]
    const uses = /^    uses: (.+)$/.exec(line)
    if (uses) current.uses = uses[1]
  }
  return jobs
}

const stableJobs = [
  "prepare",
  "generate-changelog",
  "publish-crates",
  "publish-npm",
  "publish-client-sdks",
  "build-addon",
  "smoke-addon",
  "publish-bindings",
  "build-binaries-linux-x86_64",
  "build-binaries-linux-aarch64",
  "build-binaries-darwin",
  "build-binaries-windows",
  "upload-binaries",
  "create-github-release",
  "publish-images",
  "publish-homebrew-tap",
  "publish-npm-cli-wrapper",
]

test("stable remains the default release mode", () => {
  assert.match(workflow, /mode:\n\s+description: Publication channel\n\s+type: choice\n\s+default: stable\n\s+options: \[stable, dev\]/)
})

test("dev publication requires an explicit full source commit", () => {
  assert.match(workflow, /source_ref:\n\s+description: Exact 40-character source commit/)
  const reusable = readFileSync(
    resolve(process.cwd(), ".github/workflows/publish-npm-dev.yml"),
    "utf8",
  )
  assert.match(reusable, /\^\[0-9a-f\]\{40\}\$/)
})

test("dev mode can reach only the reusable npm dev workflow", () => {
  const jobs = parseJobs(workflow)
  assert.deepEqual([...jobs.keys()].sort(), ["publish-npm-dev", ...stableJobs].sort())
  assert.equal(jobs.get("publish-npm-dev").if, "inputs.mode == 'dev'")
  assert.equal(jobs.get("publish-npm-dev").uses, "./.github/workflows/publish-npm-dev.yml")

  for (const name of stableJobs) {
    assert.match(jobs.get(name).if, /inputs\.mode == 'stable'/, `${name} must be unreachable in dev mode`)
  }
})
