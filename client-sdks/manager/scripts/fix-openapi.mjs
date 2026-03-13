#!/usr/bin/env node
/**
 * Post-processes OpenAPI 3.0 spec for progenitor compatibility
 *
 * Problem:
 *   `openapi-down-convert` converts OpenAPI 3.1 → 3.0, but leaves patterns like:
 *
 *   anyOf: [
 *     { "type": "null" },
 *     { "$ref": "#/components/schemas/SomeType" }
 *   ]
 *
 *   These patterns work in OpenAPI 3.0 spec validators, but progenitor
 *   (the Rust SDK generator) doesn't support them and fails with:
 *   "not yet implemented: invalid type: null"
 *
 * Solution:
 *   Convert these patterns to the simpler form that progenitor expects:
 *
 *   allOf: [{ "$ref": "#/components/schemas/SomeType" }]
 *   nullable: true
 *
 * This preserves the same semantics (optional field) in a format progenitor understands.
 */

import fs from "node:fs"

const INPUT_FILE = "openapi-3.0.json"

/**
 * Recursively traverse the OpenAPI spec and fix nullable patterns
 */
function fixNullablePatterns(obj) {
  if (typeof obj !== "object" || obj === null) {
    return obj
  }

  if (Array.isArray(obj)) {
    return obj.map(fixNullablePatterns)
  }

  // Recursively process all nested objects first
  const result = {}
  for (const [key, value] of Object.entries(obj)) {
    result[key] = fixNullablePatterns(value)
  }

  // Check for patterns: anyOf/oneOf with exactly 2 items, one being {"type": "null"}
  for (const combinator of ["anyOf", "oneOf"]) {
    const items = result[combinator]

    if (!Array.isArray(items) || items.length !== 2) {
      continue
    }

    // Find which item is the null type
    const nullIndex = items.findIndex(
      item => item && typeof item === "object" && item.type === "null",
    )

    if (nullIndex === -1) {
      continue
    }

    // Get the non-null schema
    const nonNullSchema = items[nullIndex === 0 ? 1 : 0]

    // Merge the non-null schema into current object and add nullable flag
    const fixed = { ...result, ...nonNullSchema, nullable: true }
    delete fixed[combinator]

    return fixed
  }

  return result
}

// Read, fix, and write back
const spec = JSON.parse(fs.readFileSync(INPUT_FILE, "utf8"))
const fixed = fixNullablePatterns(spec)

fs.writeFileSync(INPUT_FILE, JSON.stringify(fixed, null, 2), "utf8")

console.log("Fixed OpenAPI nullable patterns for progenitor compatibility")
