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
const cache = bindings.kv("alien-kv")
const key = `remote-${randomUUID()}`
const workloadUrl = required("ALIEN_WORKLOAD_URL")
async function workloadValue() {
  const response = await fetch(`${workloadUrl}/kv-remote/${key}`)
  assert.equal(response.status, 200)
  return (await response.json()).value
}
try {
  const value = { message: "remote-kv", data: "x".repeat(32000) }
  assert.equal(await cache.setJson(key, value, { ttl: 60, ifVersion: null }), true)
  assert.equal(await cache.setJson(key, {}, { ifVersion: null }), false)
  assert.deepEqual((await cache.getJson(key)).value, value)
  assert.deepEqual(await workloadValue(), value)
  assert.equal(await cache.setJson(key, { expiring: true }, { ttl: 2 }), true)
  const deadline = Date.now() + 10000
  while (await cache.exists(key)) {
    assert.ok(Date.now() < deadline, "logical TTL did not expire")
    await setTimeout(250)
  }
  assert.equal(await cache.get(key), null)
  assert.equal(await workloadValue(), null)
  assert.equal(await cache.setJson(key, { renewed: true }, { ifVersion: null }), true)
  assert.deepEqual(await workloadValue(), { renewed: true })
  console.log(
    "Remote TS KV: 32KB write, workload read, conditional write, TTL and expired-row takeover passed",
  )
} finally {
  await cache.delete(key)
}
