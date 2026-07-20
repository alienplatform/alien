import { afterEach, describe, expect, it, vi } from "vitest"
import { physicalSourceNames, sourceMatches } from "../src/worker-runtime/event-loop.js"

// Cloud transports deliver tasks keyed by the provider's physical identifier
// (S3 bucket name, SQS queue name) while handlers register by the resource's
// logical stack name; dispatch must accept both spellings via the
// ALIEN_<NAME>_BINDING env contract.
describe("event dispatch source matching", () => {
  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it("matches a storage handler registered by logical name against the bound bucket", () => {
    vi.stubEnv(
      "ALIEN_EMAIL_BINDING",
      JSON.stringify({ service: "s3", bucketName: "stack-email-8b090660" }),
    )

    expect(physicalSourceNames("email")).toEqual(["stack-email-8b090660"])
    expect(sourceMatches("email", "stack-email-8b090660")).toBe(true)
    expect(sourceMatches("email", "some-other-bucket")).toBe(false)
  })

  it("matches a queue handler registered by logical name against the queue name from queueUrl", () => {
    vi.stubEnv(
      "ALIEN_SES_EVENTS_BINDING",
      JSON.stringify({
        service: "sqs",
        queueUrl: "https://sqs.us-east-1.amazonaws.com/230470760195/stack-SesEvents-aHhH",
      }),
    )

    // '-' in the logical name maps to '_' in the env var name.
    expect(physicalSourceNames("ses-events")).toEqual(["stack-SesEvents-aHhH"])
    expect(sourceMatches("ses-events", "stack-SesEvents-aHhH")).toBe(true)
  })

  it("still matches physical names and wildcards directly", () => {
    expect(sourceMatches("stack-email-8b090660", "stack-email-8b090660")).toBe(true)
    expect(sourceMatches("*", "anything")).toBe(true)
  })

  it("does not match when the binding env var is absent or malformed", () => {
    expect(sourceMatches("email", "stack-email-8b090660")).toBe(false)

    vi.stubEnv("ALIEN_EMAIL_BINDING", "not-json")
    expect(physicalSourceNames("email")).toEqual([])
    expect(sourceMatches("email", "stack-email-8b090660")).toBe(false)
  })
})
