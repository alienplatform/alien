/**
 * Hosted Remote Bindings flow through the real napi addon. The HTTP servers
 * stand in for the public Platform API and the deployment's assigned manager;
 * all Storage operations use the real local Rust provider.
 */

import { mkdtempSync, rmSync } from "node:fs"
import { type IncomingMessage, type Server, type ServerResponse, createServer } from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { Bindings } from "../src/index.js"

const deploymentId = "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa"
const managerId = "mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb"
const projectId = "prj_cccccccccccccccccccccccccccc"
const deploymentGroupId = "dg_dddddddddddddddddddddddddddd"
const workspaceId = "ws_eeeeeeeeeeeeeeeeeeeeeeee"
const token = "remote-secret-token"

let managerServer: Server
let platformServer: Server
let managerOrigin: string
let platformOrigin: string
let storageDirectory: string
let denyRemoteAccess = false
const authorizations: Array<string | undefined> = []
const resolveBodies: unknown[] = []

function json(response: ServerResponse, status: number, body: unknown): void {
  response.writeHead(status, { "content-type": "application/json" })
  response.end(JSON.stringify(body))
}

async function bodyOf(request: IncomingMessage): Promise<unknown> {
  let body = ""
  for await (const chunk of request) body += chunk.toString()
  return body.length > 0 ? JSON.parse(body) : undefined
}

function listen(server: Server): Promise<string> {
  return new Promise((resolve, reject) => {
    server.once("error", reject)
    server.listen(0, "127.0.0.1", () => {
      const address = server.address()
      if (!address || typeof address === "string") {
        reject(new Error("fixture server did not expose a TCP address"))
        return
      }
      resolve(`http://127.0.0.1:${address.port}`)
    })
  })
}

function close(server: Server): Promise<void> {
  return new Promise((resolve, reject) => {
    server.close(error => (error ? reject(error) : resolve()))
  })
}

beforeAll(async () => {
  storageDirectory = mkdtempSync(join(tmpdir(), "alien-remote-bindings-"))
  managerServer = createServer(async (request, response) => {
    authorizations.push(request.headers.authorization)
    if (request.method !== "POST" || request.url !== "/v1/bindings/resolve") {
      json(response, 404, { message: "not found" })
      return
    }
    resolveBodies.push(await bodyOf(request))
    if (denyRemoteAccess) {
      json(response, 403, {
        code: "FORBIDDEN",
        message: "Remote access was revoked",
        retryable: false,
        internal: false,
        httpStatusCode: 403,
      })
      return
    }
    json(response, 200, {
      binding: { service: "local-storage", storagePath: storageDirectory },
      clientConfig: { platform: "local", state_directory: storageDirectory },
      expiresAt: new Date(Date.now() + 60 * 60 * 1000).toISOString(),
    })
  })
  managerOrigin = await listen(managerServer)

  platformServer = createServer((request, response) => {
    authorizations.push(request.headers.authorization)
    if (request.method === "GET" && request.url === `/v1/deployments/${deploymentId}`) {
      json(response, 200, {
        id: deploymentId,
        name: "remote-storage-test",
        status: "running",
        projectId,
        platform: "local",
        deploymentProtocolVersion: 1,
        deploymentGroupId,
        stackSettings: {},
        retryRequested: false,
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        managerId,
        workspaceId,
      })
      return
    }
    if (request.method === "GET" && request.url === `/v1/managers/${managerId}`) {
      json(response, 200, {
        id: managerId,
        name: "fixture-manager",
        targets: ["local"],
        managementConfigs: {},
        isSystem: true,
        workspaceId,
        status: "healthy",
        url: managerOrigin,
        managedDeploymentCount: 1,
        defaultProjectCount: 0,
        createdAt: "2026-01-01T00:00:00Z",
      })
      return
    }
    json(response, 404, { message: "not found" })
  })
  platformOrigin = await listen(platformServer)
})

afterAll(async () => {
  await Promise.all([close(platformServer), close(managerServer)])
  rmSync(storageDirectory, { recursive: true, force: true })
})

describe("Bindings.forRemoteDeployment (real addon)", () => {
  it("discovers, performs the v0 Storage operations, and preserves manager denial", async () => {
    const bindings = await Bindings.forRemoteDeployment({
      deploymentId,
      token,
      apiBaseUrl: platformOrigin,
    })
    const storage = bindings.storage("uploads")
    expect(bindings.storage("uploads")).toBe(storage)

    await storage.put("reports/latest.json", Buffer.from('{"ready":true}'))
    expect((await storage.get("reports/latest.json")).toString()).toBe('{"ready":true}')
    expect((await storage.head("reports/latest.json")).size).toBe(14)
    expect((await storage.list("reports")).map(object => object.location)).toEqual([
      "reports/latest.json",
    ])
    await storage.delete("reports/latest.json")
    expect(await storage.list("reports")).toEqual([])

    expect(resolveBodies).toEqual([{ deploymentId, resourceId: "uploads" }])
    expect(authorizations.every(value => value === `Bearer ${token}`)).toBe(true)

    denyRemoteAccess = true
    const deniedBindings = await Bindings.forRemoteDeployment({
      deploymentId,
      token,
      apiBaseUrl: platformOrigin,
    })
    await expect(deniedBindings.storage("uploads").head("missing.txt")).rejects.toMatchObject({
      code: "FORBIDDEN",
      message: "Remote access was revoked",
      retryable: false,
    })
  })
})
