import type { Deployment } from "@aliendotdev/testing"
import { assertCheck, assertResponseOk, failCheck } from "./errors.js"

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (typeof a !== typeof b || a === null || b === null) return false
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((v, i) => deepEqual(v, b[i]))
  }
  if (typeof a === "object" && typeof b === "object") {
    const aObj = a as Record<string, unknown>
    const bObj = b as Record<string, unknown>
    const keys = new Set([...Object.keys(aObj), ...Object.keys(bObj)])
    return [...keys].every(k => deepEqual(aObj[k], bObj[k]))
  }
  return false
}

export async function checkInspect(deployment: Deployment): Promise<void> {
  const testPayload = {
    message: "test-inspect-request",
    timestamp: new Date().toISOString(),
    nested: {
      value: 123,
      array: [1, 2, 3],
    },
  }

  const response = await fetch(`${deployment.url}/inspect`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(testPayload),
  })

  await assertResponseOk(response, "inspect", "Inspect endpoint request failed")

  const data = (await response.json()) as { success: boolean; requestBody: unknown }

  assertCheck(data.success, "inspect", "Inspect endpoint reported failure")

  if (!deepEqual(data.requestBody, testPayload)) {
    failCheck("inspect", "Inspect endpoint did not echo back the request body correctly", {
      expected: testPayload,
      received: data.requestBody,
    })
  }
}
