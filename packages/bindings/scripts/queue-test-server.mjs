import { execFileSync, spawn } from "node:child_process"
import { existsSync, readFileSync } from "node:fs"
import { join } from "node:path"
import { fileURLToPath } from "node:url"

let binary
export function prepareQueueServer() {
  binary ??= execFileSync(
    "python3",
    [fileURLToPath(new URL("../../../scripts/queue-test-server.py", import.meta.url))],
    {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
    },
  ).trim()
  return binary
}
export function startQueueServer(dir, names) {
  const process = spawn(prepareQueueServer(), [dir, ...names], { stdio: "ignore" })
  const file = join(dir, "bindings.json")
  const deadline = Date.now() + 10_000
  while (!existsSync(file)) {
    if (Date.now() > deadline) {
      process.kill()
      throw new Error("Local queue test owner did not start")
    }
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 10)
  }
  return { process, bindings: JSON.parse(readFileSync(file, "utf8")) }
}
