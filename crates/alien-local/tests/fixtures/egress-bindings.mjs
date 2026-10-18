import assert from "node:assert/strict"
import { sandbox } from "../../../../packages/bindings/dist/index.js"

const box = sandbox("egress-test")
const created = await box.create({ egress: "internet" })
try {
  const found = await box.getOrCreate(created.sandboxId, { egress: "internet" })
  assert.equal(found.created, false)
  assert.equal(found.sandbox.sandboxId, created.sandboxId)
  for (const call of [
    () => box.create({ egress: "none" }),
    () => box.getOrCreate(created.sandboxId, { egress: "none" }),
    () => box.updateEgress(created.sandboxId, "none"),
  ]) {
    await assert.rejects(
      call,
      error => error.code === "OPERATION_NOT_SUPPORTED" && error.retryable === false,
    )
  }
  await assert.rejects(
    () => box.create({ egress: { allow: [] } }),
    error => error.code === "SANDBOX_EGRESS_INVALID",
  )
  await assert.rejects(
    () => box.create({ egress: { allow: Array(4097).fill("example.com") } }),
    error => error.code === "SANDBOX_EGRESS_POLICY_TOO_LARGE" && error.retryable === false,
  )
} finally {
  await box.terminate(created.sandboxId)
}
