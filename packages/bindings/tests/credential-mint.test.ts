/**
 * Credential-mint refresh coverage through the real napi addon.
 *
 * Node executes the public package directly. Bun's native addons cannot see
 * runtime `process.env` mutations, so the same client helper runs in a Bun
 * child with the complete environment present at process start. The fake mint
 * endpoint stays in this test process and records every request.
 */

import { spawn } from "node:child_process"
import { mkdirSync, mkdtempSync, rmSync } from "node:fs"
import { createServer } from "node:http"
import type { AddressInfo } from "node:net"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { describe, expect, it } from "vitest"
import { findLocalAddon, platformTriple } from "../src/loader.js"
import {
  type CredentialMintClientResult,
  exerciseLongLivedKvHandle,
} from "./helpers/credential-mint-client.js"
import { bindingEnvVarName } from "./helpers/local-binding-env.js"

interface MintRequest {
  authorization: string | undefined
  body: unknown
  method: string | undefined
  url: string | undefined
}

function runPublicClientChild(env: NodeJS.ProcessEnv): Promise<CredentialMintClientResult> {
  return new Promise((resolve, reject) => {
    const childEntry = fileURLToPath(
      new URL("./helpers/credential-mint-client.ts", import.meta.url),
    )
    const child = spawn(process.execPath, [childEntry], { env })
    let stdout = ""
    let stderr = ""

    child.stdout.setEncoding("utf8")
    child.stdout.on("data", chunk => {
      stdout += chunk
    })
    child.stderr.setEncoding("utf8")
    child.stderr.on("data", chunk => {
      stderr += chunk
    })
    child.on("error", reject)
    child.on("close", code => {
      if (code !== 0) {
        reject(new Error(`public bindings child exited with code ${code}: ${stderr}`))
        return
      }

      try {
        resolve(JSON.parse(stdout) as CredentialMintClientResult)
      } catch (error) {
        reject(
          new Error(`public bindings child returned invalid JSON '${stdout}': ${String(error)}`),
        )
      }
    })
  })
}

async function withProcessEnv<T>(
  env: NodeJS.ProcessEnv,
  removedKeys: string[],
  operation: () => Promise<T>,
): Promise<T> {
  const touchedKeys = new Set([...Object.keys(env), ...removedKeys])
  const previous = new Map([...touchedKeys].map(key => [key, process.env[key]]))

  try {
    for (const key of removedKeys) delete process.env[key]
    for (const [key, value] of Object.entries(env)) {
      if (value === undefined) delete process.env[key]
      else process.env[key] = value
    }
    return await operation()
  } finally {
    for (const [key, value] of previous) {
      if (value === undefined) delete process.env[key]
      else process.env[key] = value
    }
  }
}

describe("credential minting through the public TypeScript binding", () => {
  it("refreshes a long-lived handle before expiry and caches the refreshed provider", async () => {
    const root = mkdtempSync(join(tmpdir(), "alien-bindings-mint-test-"))
    const requests: MintRequest[] = []
    let mintCount = 0
    const server = createServer((request, response) => {
      let body = ""
      request.setEncoding("utf8")
      request.on("data", chunk => {
        body += chunk
      })
      request.on("end", () => {
        mintCount += 1
        requests.push({
          authorization: request.headers.authorization,
          body: JSON.parse(body) as unknown,
          method: request.method,
          url: request.url,
        })

        // The first config is still valid for two minutes, but is already
        // within the resolver's five-minute refresh window. The second is
        // fresh for an hour, so later operations on the same handle reuse it.
        const lifetimeSeconds = mintCount === 1 ? 120 : 3600
        response.writeHead(200, { "content-type": "application/json" })
        response.end(
          JSON.stringify({
            clientConfig: { platform: "local", state_directory: root },
            expiresAt: new Date(Date.now() + lifetimeSeconds * 1000).toISOString(),
            principal: "local:napi-mint-test",
          }),
        )
      })
    })

    try {
      await new Promise<void>((resolve, reject) => {
        server.once("error", reject)
        server.listen(0, "127.0.0.1", resolve)
      })
      const address = server.address() as AddressInfo | null
      if (!address) throw new Error("fake mint server did not expose its address")

      const addonPath = findLocalAddon(platformTriple())
      if (!addonPath) throw new Error("real napi addon must be built before this test runs")

      const dataDir = join(root, "mint-cache")
      mkdirSync(dataDir)

      const bindingEnv: NodeJS.ProcessEnv = {
        ALIEN_BINDINGS_ADDON_PATH: addonPath,
        ALIEN_DEPLOYMENT_TYPE: "aws",
        ALIEN_MANAGER_URL: `http://127.0.0.1:${address.port}`,
        ALIEN_DEPLOYMENT_TOKEN: "napi-test-deployment-token",
        ALIEN_DEPLOYMENT_ID: "napi-test-deployment",
        ALIEN_DEPLOYMENT_SERVICE_ACCOUNT: "napi-test-service-account",
        ALIEN_RESOURCE_ID: "napi-test-resource",
        AWS_EC2_METADATA_DISABLED: "true",
        AWS_CONFIG_FILE: join(root, "missing-config"),
        AWS_PROFILE: "__alien_missing_napi_test_profile__",
        AWS_SHARED_CREDENTIALS_FILE: join(root, "missing-credentials"),
        [bindingEnvVarName("mint-cache")]: JSON.stringify({
          service: "local-kv",
          dataDir,
        }),
      }
      const removedAwsCredentialKeys = [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_WEB_IDENTITY_TOKEN_FILE",
        "AWS_ROLE_ARN",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
      ]

      const result =
        process.env.BUN_EXPECTED === "1"
          ? await runPublicClientChild({
              HOME: root,
              PATH: process.env.PATH,
              TMPDIR: process.env.TMPDIR,
              ...bindingEnv,
            })
          : await withProcessEnv(bindingEnv, removedAwsCredentialKeys, exerciseLongLivedKvHandle)

      expect(result).toEqual({ first: "first", second: "second" })
      expect(requests).toEqual([
        {
          authorization: "Bearer napi-test-deployment-token",
          body: {
            bindingName: "napi-test-service-account",
            deploymentId: "napi-test-deployment",
            resourceId: "napi-test-resource",
          },
          method: "POST",
          url: "/v1/credentials/mint",
        },
        {
          authorization: "Bearer napi-test-deployment-token",
          body: {
            bindingName: "napi-test-service-account",
            deploymentId: "napi-test-deployment",
            resourceId: "napi-test-resource",
          },
          method: "POST",
          url: "/v1/credentials/mint",
        },
      ])
    } finally {
      await new Promise<void>((resolve, reject) => {
        server.close(error => {
          if (error) reject(error)
          else resolve()
        })
      })
      rmSync(root, { force: true, recursive: true })
    }
  }, 30_000)
})
