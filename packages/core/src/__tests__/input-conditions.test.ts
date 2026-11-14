import { describe, expect, it } from "vitest"
import {
  boolean,
  getStackInputDefinitions,
  inputs,
  secret,
  stackInputApplies,
  string,
} from "../input.js"

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

  it("resolves an off-platform reference to false", () => {
    const all = definitions(false)
    expect(stackInputApplies(all[1]!, all, { enabled: false }, "aws")).toBe(true)
    expect(stackInputApplies(all[1]!, all, { enabled: true }, "kubernetes")).toBe(true)
    const positive = definitions(true)
    expect(stackInputApplies(positive[1]!, positive, { enabled: true }, "kubernetes")).toBe(false)
  })

  it("keeps legacy empty platform scopes available", () => {
    const all = definitions(false)
    all[0]!.platforms = []
    expect(stackInputApplies(all[0]!, all, {}, "aws")).toBe(true)
    expect(stackInputApplies(all[1]!, all, { enabled: true }, "aws")).toBe(false)
    expect(stackInputApplies(all[1]!, all, { enabled: false }, "aws")).toBe(true)
    const positive = definitions(true)
    positive[0]!.platforms = []
    positive[1]!.platforms = []
    expect(stackInputApplies(positive[1]!, positive, {}, "kubernetes")).toBe(true)
    expect(stackInputApplies(positive[1]!, positive, { enabled: false }, "aws")).toBe(false)
  })

  it("rejects developer-only controllers for deployer inputs and conditional generation", () => {
    expect(() =>
      inputs({
        enabled: boolean({ ...options, providedBy: "developer" }),
        bucket: string({ ...options, when: { input: "enabled", equals: true } }),
      }),
    ).toThrow("deployer-provided")
    expect(() =>
      inputs({
        enabled: boolean({ ...options, providedBy: "developer" }),
        bucket: string({
          ...options,
          providedBy: "developer",
          when: { input: "enabled", equals: true },
        }),
      }),
    ).not.toThrow()
    expect(() =>
      inputs({
        enabled: boolean({ ...options, providedBy: "developer" }),
        token: secret({
          ...options,
          providedBy: "developer",
          generate: { length: 64 },
          when: { input: "enabled", equals: true },
        }),
      }),
    ).toThrow("generate")
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
