/**
 * Hosted Remote Bindings flow through the real napi addon. The HTTP servers
 * stand in for the public Platform API and the deployment's assigned manager.
 * The request traverses discovery and the generated manager client before the
 * fixture manager returns a structured authorization denial.
 */

import { type IncomingMessage, type Server, type ServerResponse, createServer } from "node:http"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { Bindings } from "../src/index.js"

const deploymentId = "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa"
const managerId = "mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb"
const projectId = "prj_cccccccccccccccccccccccccccc"
const deploymentGroupId = "dg_dddddddddddddddddddddddddddd"
const workspaceId = "ws_eeeeeeeeeeeeeeeeeeeeeeee"
const token = "remote-secret-token"

let managerServer: Server | undefined
let platformServer: Server | undefined
let managerOrigin: string
let platformOrigin: string
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

function close(server: Server | undefined): Promise<void> {
  if (!server?.listening) return Promise.resolve()
  return new Promise((resolve, reject) => {
    server.close(error => (error ? reject(error) : resolve()))
    server.closeAllConnections()
  })
}

beforeAll(async () => {
  managerServer = createServer(async (request, response) => {
    authorizations.push(request.headers.authorization)
    if (request.method !== "POST" || request.url !== "/v1/bindings/resolve") {
      json(response, 404, { message: "not found" })
      return
    }
    resolveBodies.push(await bodyOf(request))
    json(response, 403, {
      code: "FORBIDDEN",
      message: "Remote access was revoked",
      retryable: false,
      internal: false,
      httpStatusCode: 403,
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
})

describe("Bindings.forRemoteDeployment (real addon)", () => {
  it("discovers the assigned manager and preserves its structured denial", async () => {
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
    expect(resolveBodies).toEqual([{ deploymentId, resourceId: "uploads" }])
    expect(authorizations.every(value => value === `Bearer ${token}`)).toBe(true)
  })
})
