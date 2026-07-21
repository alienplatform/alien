import assert from "node:assert/strict"

const requiredEnvironmentVariables = [
  "ALIEN_API_URL",
  "ALIEN_API_KEY",
  "ALIEN_DEPLOYMENT_ID",
  "ALIEN_STORAGE_BINDING",
]

const payload = Buffer.from("alien remote storage smoke")

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
    await storage.put(object, payload)

    const prefix = object.slice(0, object.lastIndexOf("/") + 1)
    const downloaded = await storage.get(object)
    assert.deepEqual(downloaded, payload)

    const metadata = await storage.head(object)
    assert.equal(metadata.location, object)
    assert.equal(metadata.size, payload.byteLength)

    const listed = await storage.list(prefix)
    assert.ok(
      listed.some(item => item.location === object),
      "uploaded object was absent from list",
    )

    await storage.delete(object)
    cleanupRequired = false
    await assert.rejects(storage.head(object))
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
