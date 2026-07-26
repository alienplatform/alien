import { describe, expect, it } from "vitest"

import { AwsOpenSearch } from "../experimental/aws-opensearch.js"

describe("AwsOpenSearch", () => {
  it("omits capacity by default so provider defaults remain in effect", () => {
    const resource = new AwsOpenSearch("messages").build()

    expect(resource.config.capacity).toBeUndefined()
  })

  it("builds independent indexing and search capacity ranges", () => {
    const resource = new AwsOpenSearch("messages")
      .capacity({
        indexing: { minOcu: 1, maxOcu: 8 },
        search: { minOcu: 1, maxOcu: 16 },
      })
      .build()

    expect(resource.config.capacity).toEqual({
      indexing: { minOcu: 1, maxOcu: 8 },
      search: { minOcu: 1, maxOcu: 16 },
    })
  })

  it.each([
    [{}, /must configure indexing, search, or both/],
    [{ indexing: {} }, /indexing capacity must configure/],
    [{ indexing: { minOcu: 3 } }, /indexing.minOcu must be/],
    [{ search: { maxOcu: 0 } }, /search.maxOcu must be/],
    [{ search: { minOcu: 8, maxOcu: 4 } }, /minOcu must not exceed maxOcu/],
  ])("rejects unsupported capacity %j", (capacity, message) => {
    expect(() => new AwsOpenSearch("messages").capacity(capacity).build()).toThrow(message)
  })
})
