import { Bindings } from "@alienplatform/bindings"

function requiredEnvironmentVariable(name: string): string {
  const value = process.env[name]
  if (!value) {
    throw new Error(`${name} is required`)
  }
  return value
}

const bindings = await Bindings.forRemoteDeployment({
  deploymentId: requiredEnvironmentVariable("ALIEN_DEPLOYMENT_ID"),
  token: requiredEnvironmentVariable("ALIEN_API_TOKEN"),
})

const uploads = bindings.storage("uploads")
const objectPath = "hello.txt"

await uploads.put(objectPath, new TextEncoder().encode("hello from the vendor backend"))

const head = await uploads.head(objectPath)
const object = await uploads.get(objectPath)
const objects = await uploads.list()

console.log({
  head,
  contents: new TextDecoder().decode(object.data),
  objects,
})

await uploads.delete(objectPath)
