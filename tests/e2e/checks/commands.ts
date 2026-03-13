import type { Deployment } from "@alienplatform/testing"
import { assertCheck } from "./errors.js"

export async function checkCommandEcho(agent: Deployment): Promise<void> {
  const testParams = {
    message: "test-echo-command",
    timestamp: new Date().toISOString(),
  }
  const result = await agent.invokeCommand("echo", testParams)
  assertCheck(
    JSON.stringify(result) === JSON.stringify(testParams),
    "command-echo",
    "Echo command did not return params unchanged",
  )
}

export async function checkCommandSmallPayload(agent: Deployment): Promise<void> {
  const testParams = {
    testType: "small-payload",
    data: `test-data-${Date.now()}`,
  }
  const result = await agent.invokeCommand("arc-test-small", testParams)
  assertCheck(result.success, "command-small-payload", "ARC small payload test reported failure")
  assertCheck(
    result.testType === "arc-small-payload",
    "command-small-payload",
    "Unexpected test type",
    {
      actualType: result.testType,
    },
  )
  assertCheck(
    Boolean(result.paramsHash),
    "command-small-payload",
    "ARC small payload response missing paramsHash",
  )
}

export async function checkCommandLargePayload(agent: Deployment): Promise<void> {
  const testParams = {
    testType: "large-payload",
    data: `test-data-${Date.now()}`,
  }
  const result = await agent.invokeCommand("arc-test-large", testParams)
  assertCheck(result.success, "command-large-payload", "ARC large payload test reported failure")
  assertCheck(
    result.testType === "arc-large-payload",
    "command-large-payload",
    "Unexpected test type",
    {
      actualType: result.testType,
    },
  )
  assertCheck(
    result.bulkData && Array.isArray(result.bulkData),
    "command-large-payload",
    "ARC large payload response missing or invalid bulkData array",
  )
  assertCheck(
    result.bulkData.length >= 1000,
    "command-large-payload",
    "ARC large payload response too small",
    {
      actualLength: result.bulkData.length,
    },
  )
}

export async function checkCommands(agent: Deployment): Promise<void> {
  await checkCommandEcho(agent)
  await checkCommandSmallPayload(agent)
  await checkCommandLargePayload(agent)
}
