#!/usr/bin/env node

import { readFileSync, writeFileSync } from "node:fs"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"

const scriptDir = fileURLToPath(new URL(".", import.meta.url))
const urlModule = resolve(scriptDir, "../typescript/src/lib/url.ts")
const paramMarker = '  const paramRE = /\\{([a-zA-Z0-9_][a-zA-Z0-9_-]*?)\\}/g;\n'
const returnMarker = "    return pathPattern.replace(paramRE, function"
const fixedReturnMarker = "    return relativePathPattern.replace(paramRE, function"
const source = readFileSync(urlModule, "utf8")

if (source.includes(fixedReturnMarker)) process.exit(0)

if (!source.includes(paramMarker) || !source.includes(returnMarker)) {
  throw new Error("The generated URL helper no longer matches the reviewed post-processing input")
}

const fixed = source
  .replace(
    paramMarker,
    `${paramMarker}\n  const relativePathPattern = pathPattern.replace(/^\\/+/, "");\n`,
  )
  .replace(returnMarker, "    return relativePathPattern.replace(paramRE, function")

writeFileSync(urlModule, fixed, "utf8")
