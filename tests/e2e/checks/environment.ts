import type { Deployment } from "@alienplatform/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

export async function checkEnvironmentVariable(agent: Deployment): Promise<void> {
  // Check NODE_ENV which is explicitly set in the config
  const varName = "NODE_ENV"
  const response = await fetch(`${agent.url}/env-var/${varName}`)
  await assertResponseOk(
    response,
    "environment-variable",
    "Environment variable check request failed",
  )
  const data = (await response.json()) as { name: string; value: string }
  assertCheck(Boolean(data.value), "environment-variable", "Environment variable value is empty")
  assertCheck(
    data.name === varName,
    "environment-variable",
    "Unexpected variable name in response",
    {
      expectedName: varName,
      actualName: data.name,
    },
  )
}
