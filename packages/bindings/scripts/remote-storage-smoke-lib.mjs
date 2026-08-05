import assert from "node:assert/strict"

const requiredEnvironmentVariables = [
  "ALIEN_API_URL",
  "ALIEN_API_KEY",
  "ALIEN_DEPLOYMENT_ID",
  "ALIEN_STORAGE_BINDING",
]

const payload = Buffer.from("alien remote storage smoke")
const attributes = {
  contentType: "application/octet-stream",
  contentDisposition: 'attachment; filename="payload.txt"',
  cacheControl: "private, max-age=60",
  metadata: { source: "remote-storage-smoke" },
}

/**
 * @typedef {object} RemoteStorageSmokeConfig
 * @property {string} apiUrl
 * @property {string} apiKey
 * @property {string} deploymentId
 * @property {string} storageBinding
 */

/**
 * @param {Readonly<Record<string, string | undefined>>} environment
 * @returns {RemoteStorageSmokeConfig}
 */
export function readRemoteStorageSmokeConfig(environment) {
  const missing = requiredEnvironmentVariables.filter(name => !environment[name]?.trim())
  if (missing.length > 0) {
    throw new Error(`Missing required environment variables: ${missing.join(", ")}`)
  }

  const required = name => {
    const value = environment[name]?.trim()
    if (!value) throw new Error(`${name} is required`)
    return value
  }
  return {
    apiUrl: required("ALIEN_API_URL"),
    apiKey: required("ALIEN_API_KEY"),
    deploymentId: required("ALIEN_DEPLOYMENT_ID"),
    storageBinding: required("ALIEN_STORAGE_BINDING"),
  }
}

/**
 * @param {import("../dist/index.js").RemoteStorage} storage
 * @param {string} object
 */
export async function verifyRemoteStorage(storage, object) {
  let cleanupRequired = true
  let verificationError

  try {
    const putResult = await storage.put(object, payload, { attributes })
    assert.equal(typeof putResult, "object")

    const prefix = object.slice(0, object.lastIndexOf("/") + 1)
    const downloaded = await storage.get(object)
    assert.deepEqual(downloaded.data, payload)
    assert.equal(downloaded.meta.location, object)
    assert.equal(downloaded.meta.size, payload.byteLength)
    assert.equal(downloaded.attributes.contentType, attributes.contentType)
    assert.equal(downloaded.attributes.contentDisposition, attributes.contentDisposition)
    assert.equal(downloaded.attributes.cacheControl, attributes.cacheControl)
    assert.deepEqual(downloaded.attributes.metadata, attributes.metadata)

    const head = await storage.head(object)
    assert.equal(head.meta.location, object)
    assert.equal(head.meta.size, payload.byteLength)
    assert.equal(head.attributes.contentType, attributes.contentType)
    assert.equal(head.attributes.contentDisposition, attributes.contentDisposition)
    assert.equal(head.attributes.cacheControl, attributes.cacheControl)
    assert.deepEqual(head.attributes.metadata, attributes.metadata)

    const listed = await storage.list(prefix)
    assert.ok(
      listed.some(item => item.location === object),
      "uploaded object was absent from list",
    )

    await storage.delete(object)
    cleanupRequired = false
    const listedAfterDelete = await storage.list(prefix)
    assert.ok(
      !listedAfterDelete.some(item => item.location === object),
      "deleted object remained in list",
    )
  } catch (error) {
    verificationError = error
  }

  let cleanupError
  if (cleanupRequired) {
    try {
      await storage.delete(object)
    } catch (error) {
      cleanupError = error
    }
  }

  if (verificationError !== undefined && cleanupError !== undefined) {
    throw new AggregateError(
      [verificationError, cleanupError],
      "remote Storage verification and cleanup failed",
    )
  }
  if (verificationError !== undefined) throw verificationError
  if (cleanupError !== undefined) throw cleanupError
}
