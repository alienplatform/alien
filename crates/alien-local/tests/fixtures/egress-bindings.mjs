import assert from "node:assert/strict"
import { sandbox } from "../../../../packages/bindings/dist/index.js"

const box = sandbox("egress-test")
const created = await box.create()
try {
  const found = await box.getOrCreate(created.sandboxId)
  assert.equal(found.created, false)
  assert.equal(found.sandbox.sandboxId, created.sandboxId)
  for (const call of [
    () => box.create({ egress: "internet" }),
    () => box.create({ egress: { allow: [] } }),
    () => box.getOrCreate(created.sandboxId, { egress: { allow: [] } }),
    () => box.updateEgress(created.sandboxId, { allow: [] }),
  ]) {
    await assert.rejects(
      call,
      error => error.code === "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE" && error.retryable === false,
    )
  }
  await assert.rejects(
    () => box.create({ egress: { allow: ["169.254.169.254"] } }),
    error => error.code === "SANDBOX_EGRESS_INVALID",
  )
  await assert.rejects(
    () => box.create({ egress: { allow: Array(4097).fill("example.com") } }),
    error => error.code === "SANDBOX_EGRESS_POLICY_TOO_LARGE" && error.retryable === false,
  )
} finally {
  await box.terminate(created.sandboxId)
}
