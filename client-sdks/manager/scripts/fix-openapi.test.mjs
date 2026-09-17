import assert from "node:assert/strict"
import test from "node:test"

import { fixNullablePatterns } from "./fix-openapi.mjs"

test("wraps nullable references in an OpenAPI 3.0 schema object", () => {
  assert.deepEqual(
    fixNullablePatterns({
      description: "optional image receipt",
      anyOf: [{ type: "null" }, { $ref: "#/components/schemas/OperatorImageReport" }],
    }),
    {
      description: "optional image receipt",
      nullable: true,
      allOf: [{ $ref: "#/components/schemas/OperatorImageReport" }],
    },
  )
})

test("keeps inline nullable schemas inline", () => {
  assert.deepEqual(
    fixNullablePatterns({ oneOf: [{ type: "string", maxLength: 64 }, { type: "null" }] }),
    { type: "string", maxLength: 64, nullable: true },
  )
})

test("fixes nullable references recursively", () => {
  assert.deepEqual(
    fixNullablePatterns({
      properties: {
        value: { anyOf: [{ $ref: "#/components/schemas/Value" }, { type: "null" }] },
      },
    }),
    {
      properties: {
        value: {
          nullable: true,
          allOf: [{ $ref: "#/components/schemas/Value" }],
        },
      },
    },
  )
})
