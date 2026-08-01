import { randomUUID } from "node:crypto"
import { Bindings } from "../dist/index.js"
import { readRemoteStorageSmokeConfig, verifyRemoteStorage } from "./remote-storage-smoke-lib.mjs"

const config = readRemoteStorageSmokeConfig(process.env)
const bindings = await Bindings.forRemoteDeployment({
  apiBaseUrl: config.apiUrl,
  deploymentId: config.deploymentId,
  token: config.apiKey,
})
const storage = bindings.storage(config.storageBinding)
const object = `alien-e2e/remote-storage-smoke/${randomUUID()}/payload.txt`

await verifyRemoteStorage(storage, object)
console.log("Remote Storage put/get/head/list/delete smoke passed")
