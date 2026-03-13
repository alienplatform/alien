import type { Deployment } from "@alienplatform/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

const BINDING_NAME = "test-alien-kv"

export async function checkKV(agent: Deployment): Promise<void> {
  const response = await fetch(`${agent.url}/kv-test/${BINDING_NAME}`, {
    method: "POST",
  })
  await assertResponseOk(response, "kv", "KV test request failed")
  const data = (await response.json()) as { success: boolean; bindingName: string }
  assertCheck(data.success, "kv", "KV test reported failure")
  assertCheck(data.bindingName === BINDING_NAME, "kv", "Unexpected binding name in response", {
    expectedBinding: BINDING_NAME,
    actualBinding: data.bindingName,
  })
}
