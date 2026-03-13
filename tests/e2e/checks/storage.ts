import type { Deployment } from "@alienplatform/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

const BINDING_NAME = "test-alien-storage"

export async function checkStorage(agent: Deployment): Promise<void> {
  const response = await fetch(`${agent.url}/storage-test/${BINDING_NAME}`, {
    method: "POST",
  })
  await assertResponseOk(response, "storage", "Storage test request failed")
  const data = (await response.json()) as { success: boolean; bindingName: string }
  assertCheck(data.success, "storage", "Storage test reported failure")
  assertCheck(data.bindingName === BINDING_NAME, "storage", "Unexpected binding name in response", {
    expectedBinding: BINDING_NAME,
    actualBinding: data.bindingName,
  })
}
