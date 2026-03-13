import type { Deployment } from "@alienplatform/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

const BINDING_NAME = "test-alien-vault"

export async function checkVault(agent: Deployment): Promise<void> {
  const response = await fetch(`${agent.url}/vault-test/${BINDING_NAME}`, {
    method: "POST",
  })
  await assertResponseOk(response, "vault", "Vault test request failed")
  const data = (await response.json()) as { success: boolean; bindingName: string }
  assertCheck(data.success, "vault", "Vault test reported failure")
  assertCheck(data.bindingName === BINDING_NAME, "vault", "Unexpected binding name in response", {
    expectedBinding: BINDING_NAME,
    actualBinding: data.bindingName,
  })
}
