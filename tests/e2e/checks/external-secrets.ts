import type { Deployment } from "@aliendotdev/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

export async function checkExternalSecret(deployment: Deployment): Promise<void> {
  const testValue = `external-${Date.now()}`

  // Set secret using platform-native tools (SSM/Secret Manager/Key Vault/etc)
  await deployment.setExternalSecret("test-alien-vault", "EXTERNAL_TEST_SECRET", testValue)

  // Wait for propagation
  await new Promise(resolve => setTimeout(resolve, 2000))

  // Verify the deployment can read it
  const response = await fetch(`${deployment.url}/external-secret`)

  await assertResponseOk(response, "external-secret", "External secret endpoint request failed")

  const data = (await response.json()) as { exists: boolean; value?: string }

  assertCheck(data.exists, "external-secret", "External secret not found")

  assertCheck(data.value === testValue, "external-secret", "External secret value mismatch", {
    expectedValue: testValue,
    actualValue: data.value,
  })
}
