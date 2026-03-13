import type { Deployment } from "@aliendotdev/testing"
import { assertCheck, assertResponseOk } from "./errors.js"

export async function checkHealth(app: Deployment): Promise<void> {
  const response = await fetch(`${app.url}/health`)
  await assertResponseOk(response, "health", "Health check request failed")
  const data = (await response.json()) as { status: string }
  assertCheck(data.status === "ok", "health", "Health check returned unexpected status", {
    status: data.status,
  })
}

export async function checkHello(app: Deployment): Promise<void> {
  const response = await fetch(`${app.url}/hello`)
  await assertResponseOk(response, "hello", "Hello endpoint request failed")
  const text = await response.text()
  assertCheck(text.includes("Hello"), "hello", "Hello endpoint returned unexpected response", {
    responseText: text,
  })
}
