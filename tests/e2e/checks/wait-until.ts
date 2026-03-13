import type { Deployment } from "@alienplatform/testing"
import { assertCheck, assertResponseOk, failCheck } from "./errors.js"

const STORAGE_BINDING_NAME = "test-alien-storage"
const DELAY_MS = 2000
const VERIFICATION_WAIT_MS = 5000
const MAX_VERIFICATION_ATTEMPTS = 6
const RETRY_DELAY_MS = 5000

interface WaitUntilTestResponse {
  success: boolean
  testId: string
  message: string
  error?: string
}

interface WaitUntilVerifyResponse {
  success: boolean
  testId: string
  backgroundTaskCompleted: boolean
  fileContent?: string
  error?: string
  message: string
}

export async function checkWaitUntil(deployment: Deployment): Promise<void> {
  const testData = `wait-until-test-data-${Date.now()}`

  // Step 1: Trigger background task
  const triggerResponse = await fetch(`${deployment.url}/wait-until-test`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      storageBindingName: STORAGE_BINDING_NAME,
      testData,
      delayMs: DELAY_MS,
    }),
  })

  await assertResponseOk(triggerResponse, "wait-until-trigger", "Wait-until trigger request failed")

  const triggerData = (await triggerResponse.json()) as WaitUntilTestResponse

  assertCheck(triggerData.success, "wait-until-trigger", "Wait-until trigger reported failure", {
    error: triggerData.error,
  })

  const testId = triggerData.testId

  // Step 2: Wait then verify with retries
  await new Promise(resolve => setTimeout(resolve, VERIFICATION_WAIT_MS))

  for (let attempt = 1; attempt <= MAX_VERIFICATION_ATTEMPTS; attempt++) {
    const verifyResponse = await fetch(
      `${deployment.url}/wait-until-verify/${testId}/${STORAGE_BINDING_NAME}`,
    )

    await assertResponseOk(
      verifyResponse,
      "wait-until-verify",
      "Wait-until verification request failed",
    )

    const verifyData = (await verifyResponse.json()) as WaitUntilVerifyResponse

    if (verifyData.backgroundTaskCompleted && verifyData.success) {
      assertCheck(
        verifyData.fileContent === testData,
        "wait-until-verify",
        "Background task completed but content doesn't match",
        {
          expectedContent: testData,
          actualContent: verifyData.fileContent,
        },
      )
      return
    }

    if (attempt < MAX_VERIFICATION_ATTEMPTS) {
      await new Promise(resolve => setTimeout(resolve, RETRY_DELAY_MS))
    } else {
      failCheck("wait-until-verify", "Background task did not complete in time", {
        maxAttempts: MAX_VERIFICATION_ATTEMPTS,
        lastMessage: verifyData.message,
      })
    }
  }
}
