import type { Deployment } from "@aliendotdev/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

const BINDING_NAME = "test-alien-queue"

export async function checkQueue(agent: Deployment): Promise<void> {
  const response = await fetch(`${agent.url}/queue-test/${BINDING_NAME}`, {
    method: "POST",
  })
  await assertResponseOk(response, "queue", "Queue test request failed")
  const data = (await response.json()) as { success: boolean; bindingName: string }
  assertCheck(data.success, "queue", "Queue test reported failure")
  assertCheck(data.bindingName === BINDING_NAME, "queue", "Unexpected binding name in response", {
    expectedBinding: BINDING_NAME,
    actualBinding: data.bindingName,
  })
}
