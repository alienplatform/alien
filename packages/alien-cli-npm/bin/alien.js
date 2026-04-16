#!/usr/bin/env node

import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { createRequire } from "node:module"
import { dirname, join } from "node:path"

// Map Node.js platform/arch to Rust target triples
const TARGET_MAP = {
  "linux-x64": "x86_64-unknown-linux-musl",
  "linux-arm64": "aarch64-unknown-linux-musl",
  "darwin-arm64": "aarch64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
}

const platformKey = `${process.platform}-${process.arch}`
const target = TARGET_MAP[platformKey]

if (!target) {
  console.error(`Unsupported platform: ${platformKey}`)
  process.exit(1)
}

const binaryName = process.platform === "win32" ? "alien.exe" : "alien"

// Try to find the binary from the platform-specific optional dependency
function findBinary() {
  const require = createRequire(import.meta.url)

  // Strategy 1: Look for the platform-specific optional dependency
  const platformPkg = `@alienplatform/cli-${platformKey}`
  try {
    const pkgPath = require.resolve(`${platformPkg}/package.json`)
    const vendorDir = join(dirname(pkgPath), "vendor", target)
    const binPath = join(vendorDir, binaryName)
    if (existsSync(binPath)) return binPath
  } catch {
    // Not installed (expected on other platforms)
  }

  // Strategy 2: Look in local vendor directory (for development / non-npm installs)
  const localVendor = join(dirname(import.meta.url.replace("file://", "")), "..", "vendor", target)
  const localBin = join(localVendor, binaryName)
  if (existsSync(localBin)) return localBin

  return null
}

const binPath = findBinary()

if (!binPath) {
  console.error(
    `Could not find alien binary for ${platformKey}.\nTry reinstalling: npm install -g @alienplatform/cli`,
  )
  process.exit(1)
}

// Spawn the binary, forwarding all arguments and stdio
const child = spawn(binPath, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
})

// Forward signals to the child process
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    if (!child.killed) {
      child.kill(signal)
    }
  })
}

child.on("close", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
  } else {
    process.exit(code ?? 1)
  }
})
