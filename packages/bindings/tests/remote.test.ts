/**
 * Hosted Remote Bindings flow through the real napi addon. The HTTP servers
 * stand in for the public Platform API and the deployment's assigned manager.
 * The request traverses discovery and the generated manager client before the
 * fixture manager returns a structured authorization denial.
 */

import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { Bindings } from "../src/index.js"

const deploymentId = "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa"
const managerId = "mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb"
const projectId = "prj_cccccccccccccccccccccccccccc"
const deploymentGroupId = "dg_dddddddddddddddddddddddddddd"
const workspaceId = "ws_eeeeeeeeeeeeeeeeeeeeeeee"
const token = "remote-secret-token"
const bindingToken = "manager-binding-token"

let managerServer: Server | undefined
let platformServer: Server | undefined
let managerOrigin: string
let platformOrigin: string
const platformAuthorizations: Array<string | undefined> = []
const managerAuthorizations: Array<string | undefined> = []
const bindingTokenBodies: unknown[] = []
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
    managerAuthorizations.push(request.headers.authorization)
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

  platformServer = createServer(async (request, response) => {
    platformAuthorizations.push(request.headers.authorization)
    if (request.method === "GET" && request.url === `/v1/deployments/${deploymentId}`) {
      json(response, 200, {
        id: deploymentId,
        name: "remote-storage-test",
        status: "running",
        projectId,
        platform: "local",
        deploymentProtocolVersion: 1,
        deploymentGroupId,
        purpose: "storage",
        releaseChannel: "production",
        stackSettings: {},
        retryRequested: false,
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        managerId,
        workspaceId,
      })
      return
    }
    if (request.method === "POST" && request.url === `/v1/managers/${managerId}/binding-token`) {
      bindingTokenBodies.push(await bodyOf(request))
      json(response, 200, {
        accessToken: bindingToken,
        expiresIn: 300,
        tokenType: "Bearer",
        managerUrl: managerOrigin,
        databaseId: null,
        controlPlaneUrl: null,
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
    // A manager-side authorization rejection refreshes discovery once before
    // preserving the second structured denial for the caller.
    expect(bindingTokenBodies).toEqual([{ deploymentId }, { deploymentId }])
    expect(resolveBodies).toEqual([
      { deploymentId, resourceId: "uploads" },
      { deploymentId, resourceId: "uploads" },
    ])
    expect(platformAuthorizations).toEqual(Array(4).fill(`Bearer ${token}`))
    expect(managerAuthorizations).toEqual(Array(2).fill(`Bearer ${bindingToken}`))
  })
})

const externalId = "ext_customer_01"
const customerToken = "remote-customer-token"
const customerBindingToken = "customer-binding-token"

let customerManagerServer: Server | undefined
let customerPlatformServer: Server | undefined
let customerManagerOrigin: string
let customerPlatformOrigin: string
const customerPlatformAuthorizations: Array<string | undefined> = []
const customerManagerAuthorizations: Array<string | undefined> = []
const externalAccessBodies: unknown[] = []
const customerResolveBodies: unknown[] = []

describe("Bindings.forRemoteCustomer (real addon)", () => {
  beforeAll(async () => {
    customerManagerServer = createServer(async (request, response) => {
      customerManagerAuthorizations.push(request.headers.authorization)
      if (request.method !== "POST" || request.url !== "/v1/bindings/resolve") {
        json(response, 404, { message: "not found" })
        return
      }
      customerResolveBodies.push(await bodyOf(request))
      json(response, 403, {
        code: "FORBIDDEN",
        message: "Remote access was revoked",
        retryable: false,
        internal: false,
        httpStatusCode: 403,
      })
    })
    customerManagerOrigin = await listen(customerManagerServer)

    customerPlatformServer = createServer(async (request, response) => {
      customerPlatformAuthorizations.push(request.headers.authorization)
      if (
        request.method === "POST" &&
        request.url === `/v1/projects/${projectId}/remote-bindings/access`
      ) {
        externalAccessBodies.push(await bodyOf(request))
        json(response, 200, {
          deploymentId,
          resourceId: "uploads",
          accessToken: customerBindingToken,
          expiresIn: 300,
          tokenType: "Bearer",
          managerUrl: customerManagerOrigin,
        })
        return
      }
      json(response, 404, { message: "not found" })
    })
    customerPlatformOrigin = await listen(customerPlatformServer)
  })

  afterAll(async () => {
    await Promise.all([close(customerPlatformServer), close(customerManagerServer)])
  })

  it("selects the customer's deployment by external ID and preserves the manager's denial", async () => {
    const bindings = await Bindings.forRemoteCustomer({
      project: projectId,
      externalId,
      token: customerToken,
      apiBaseUrl: customerPlatformOrigin,
    })
    await expect(bindings.storage("uploads").head("missing.txt")).rejects.toMatchObject({
      code: "FORBIDDEN",
      message: "Remote access was revoked",
      retryable: false,
    })
    // The rejection reselects by external ID rather than by deployment id, so the
    // second access request is the same one.
    expect(externalAccessBodies).toEqual([
      { externalId, capability: "storage" },
      { externalId, capability: "storage" },
    ])
    // The caller never supplies a deployment id on this path, so its presence in
    // the resolve body proves the manager was reached through the selection response.
    expect(customerResolveBodies).toEqual([
      { deploymentId, resourceId: "uploads" },
      { deploymentId, resourceId: "uploads" },
    ])
    expect(customerPlatformAuthorizations).toEqual(Array(2).fill(`Bearer ${customerToken}`))
    expect(customerManagerAuthorizations).toEqual(Array(2).fill(`Bearer ${customerBindingToken}`))
  })
})
