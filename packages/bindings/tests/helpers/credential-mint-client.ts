import { fileURLToPath } from "node:url"
import { kv } from "../../src/index.js"

export interface CredentialMintClientResult {
  first: string | null
  second: string | null
}

/** Exercise multiple operations through one long-lived public binding handle. */
export async function exerciseLongLivedKvHandle(): Promise<CredentialMintClientResult> {
  const cache = kv("mint-cache")

  await cache.set("key", "first")
  const first = await cache.getText("key")

  await cache.set("key", "second")
  const second = await cache.getText("key")

  return { first, second }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  exerciseLongLivedKvHandle()
    .then(result => process.stdout.write(JSON.stringify(result)))
    .catch(error => {
      console.error(error)
      process.exitCode = 1
    })
}
