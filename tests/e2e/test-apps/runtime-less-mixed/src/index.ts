import { kv } from "@alienplatform/bindings"
import { createCommandReceiver } from "@alienplatform/commands"

const RESOURCE = "typescript-container"
const ownKeys = Array.from({ length: 4 }, (_, index) => `typescript:${index}`)
const index = kv("index")

async function countExisting(keys: string[]): Promise<number> {
  const values = await Promise.all(keys.map(key => index.get(key)))
  return values.filter(value => value !== null).length
}

async function main(): Promise<void> {
  for (const key of ownKeys) {
    await index.set(key, `seeded by ${RESOURCE}`)
  }

  const receiver = createCommandReceiver()
  receiver.handle("status", async () => ({
    resource: RESOURCE,
    role: "container",
    language: "typescript",
    model: "pull",
    ownDocuments: await countExisting(ownKeys),
  }))

  await receiver.run()
}

void main()
