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
 *
 * It also removes error responses from every operation (see
 * `removeErrorResponses`), so the Rust client reads every error body itself.
 */

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

/**
 * Recursively traverse the OpenAPI spec and fix nullable patterns
 */
export function fixNullablePatterns(obj) {
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

    const fixed = { ...result, nullable: true }
    delete fixed[combinator]

    if (nonNullSchema && typeof nonNullSchema === "object" && "$ref" in nonNullSchema) {
      const { $ref, ...siblings } = nonNullSchema
      Object.assign(fixed, siblings)
      fixed.allOf = [...(Array.isArray(fixed.allOf) ? fixed.allOf : []), { $ref }]
    } else {
      Object.assign(fixed, nonNullSchema)
    }

    return fixed
  }

  return result
}

/**
 * Remove every non-2xx response from every operation.
 *
 * The manager answers every failure with an Alien error JSON body, but
 * progenitor handles a declared error status badly: a status declared without
 * a body becomes `Error::ErrorResponse` with the body thrown away, and a status
 * declared with a body turns a non-JSON body (a proxy's HTML 502) into
 * `Error::InvalidResponsePayload`, which loses the status. An undeclared status
 * reaches the caller as `Error::UnexpectedResponse` with the status and the
 * unread body, which `alien_manager_api::SdkResultExt::into_sdk_error` turns
 * into the server's own error, or a status-only error for a non-JSON body.
 * Removing them here covers every route, including routes added later.
 */
export function removeErrorResponses(spec) {
  for (const pathItem of Object.values(spec.paths ?? {})) {
    for (const operation of Object.values(pathItem)) {
      if (!operation || typeof operation !== "object" || !operation.responses) continue
      for (const status of Object.keys(operation.responses)) {
        if (!status.startsWith("2")) delete operation.responses[status]
      }
    }
  }
  return spec
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const inputFiles = process.argv.slice(2)
  if (inputFiles.length === 0) inputFiles.push("openapi-3.0.json")

  for (const inputFile of inputFiles) {
    const spec = JSON.parse(fs.readFileSync(inputFile, "utf8"))
    const fixed = removeErrorResponses(fixNullablePatterns(spec))
    fs.writeFileSync(inputFile, JSON.stringify(fixed, null, 2), "utf8")
    console.log(`Fixed OpenAPI nullable patterns and error responses in ${inputFile}`)
  }
}
