import { describe, expect, it } from "vitest"
import { boolean, getStackInputDefinitions, inputs, stackInputApplies, string } from "../input.js"

const options = {
  providedBy: "deployer" as const,
  required: true,
  label: "Input",
  description: "Input value",
}

function definitions(equals = true) {
  return getStackInputDefinitions(
    inputs({
      enabled: boolean({ ...options, default: true, platforms: ["aws"] }),
      bucket: string({ ...options, when: { input: "enabled", equals }, env: "BUCKET" }),
    }),
  )
}

describe("conditional inputs", () => {
  it("uses provided boolean values before defaults", () => {
    const all = definitions()
    const bucket = all[1]!
    expect(stackInputApplies(bucket, all, {}, "aws")).toBe(true)
    expect(stackInputApplies(bucket, all, { enabled: false }, "aws")).toBe(false)
    expect(stackInputApplies(bucket, all, { enabled: "false", bucket: "ignored" }, "aws")).toBe(
      false,
    )
    expect(stackInputApplies(bucket, all, { enabled: "true" }, "aws")).toBe(true)
  })

  it("excludes an off-platform reference even when equals is false", () => {
    const all = definitions(false)
    expect(stackInputApplies(all[1]!, all, { enabled: false }, "aws")).toBe(true)
    expect(stackInputApplies(all[1]!, all, { enabled: false }, "kubernetes")).toBe(false)
  })

  it.each(["missing", "bucket"])("rejects the reference %s", reference => {
    expect(() =>
      inputs({ bucket: string({ ...options, when: { input: reference, equals: true } }) }),
    ).toThrow()
  })

  it("rejects non-boolean references and conditional chains", () => {
    expect(() =>
      inputs({
        enabled: string(options),
        bucket: string({ ...options, when: { input: "enabled", equals: true } }),
      }),
    ).toThrow("boolean")
    expect(() =>
      inputs({
        outer: boolean(options),
        enabled: boolean({ ...options, when: { input: "outer", equals: true } }),
        bucket: string({ ...options, when: { input: "enabled", equals: true } }),
      }),
    ).toThrow("unconditional")
  })
})
