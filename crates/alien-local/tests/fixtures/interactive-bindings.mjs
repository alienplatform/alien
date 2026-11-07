import assert from "node:assert/strict"
import { sandbox } from "../../../../packages/bindings/dist/index.js"

// Drives an interactive command through the built addon against the real Local route.
const box = sandbox("interactive-test")
assert.ok((await box.capabilities()).includes("interactiveCommands"))
const created = await box.create()
try {
  const command = await box.runInteractive(created.sandboxId, "/bin/sh", {
    args: [
      "-c",
      "trap 'echo bye; exit 130' INT; printf '> '; while read l; do echo \"=$l\"; printf '> '; done",
    ],
    timeoutMs: 60_000,
  })
  let stdout = ""
  const frames = command[Symbol.asyncIterator]()
  const until = async expected => {
    while (!stdout.endsWith(expected)) {
      const { done, value } = await frames.next()
      assert.ok(!done, `ended before ${JSON.stringify(expected)}: ${JSON.stringify(stdout)}`)
      if (value.kind === "stdout") stdout += value.data.toString()
    }
  }
  await until("> ")
  await command.write("node\n")
  await until("=node\n> ")
  await command.signal("interrupt")
  let exit = null
  for (;;) {
    const { done, value } = await frames.next()
    if (done) break
    if (value.kind === "stdout") stdout += value.data.toString()
    if (value.kind === "exit") exit = value.exitCode
  }
  assert.equal(exit, 130)
  assert.ok(stdout.endsWith("bye\n"), stdout)
  await assert.rejects(
    () => command.signal("kill"),
    error => error.code === "SANDBOX_COMMAND_FAILED",
  )
} finally {
  await box.terminate(created.sandboxId)
}
