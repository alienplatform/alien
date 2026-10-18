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
      error => error.code === "SANDBOX_EGRESS_REQUIRES_INTERNET" && error.retryable === false,
    )
  }
} finally {
  await box.terminate(created.sandboxId)
}

await assert.rejects(
  () => sandbox("egress-unsupported").create(),
  error => error.code === "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE" && error.retryable === false,
)
