import assert from "node:assert/strict"
import { randomUUID } from "node:crypto"
import { setTimeout } from "node:timers/promises"
import { Bindings } from "../dist/index.js"

const required = name => {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required`)
  return value
}
const bindings = await Bindings.forRemoteDeployment({
  deploymentId: required("ALIEN_DEPLOYMENT_ID"),
  token: required("ALIEN_API_KEY"),
  apiBaseUrl: required("ALIEN_API_URL"),
})
const queue = bindings.queue("alien-events-queue")
assert.equal("receive" in queue, false)
assert.equal("ack" in queue, false)
const marker = randomUUID()
const values = Array.from({ length: 13 }, (_, index) => ({ marker, index }))
await queue.send(values[0])
const outcomes = await queue.sendBatch(values.slice(1))
assert.deepEqual(outcomes, values.slice(1).map(() => ({ status: "sent" })))
const deadline = Date.now() + 90000
const seen = new Set()
while (seen.size < values.length) {
  const response = await fetch(`${required("ALIEN_WORKLOAD_URL")}/events/list`, { signal: AbortSignal.timeout(15000) })
  assert.equal(response.status, 200)
  const { queueMessages } = await response.json()
  for (const message of queueMessages) {
    const value = JSON.parse(message.payload)
    if (value.marker === marker) {
      assert.deepEqual(value, values[value.index])
      seen.add(value.index)
    }
  }
  assert.ok(Date.now() < deadline, `workload received ${seen.size}/${values.length} remote messages`)
  if (seen.size < values.length) await setTimeout(1000)
}
console.log("Remote TS Queue: single send and 12-message batch delivered to the real workload queue trigger; all 13 payloads matched")
