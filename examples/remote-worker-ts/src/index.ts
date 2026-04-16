import { command, storage } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

const tools: Record<string, { description: string; execute: (params: any) => Promise<any> }> = {
  "read-file": {
    description: "Read a file from the customer's private workspace",
    execute: async ({ path }: { path: string }) => {
      const store = await storage("files")
      const { data } = await store.get(path)
      return { content: new TextDecoder().decode(data) }
    },
  },
  "write-file": {
    description: "Write a file to the customer's private workspace",
    execute: async ({ path, content }: { path: string; content: string }) => {
      const store = await storage("files")
      await store.put(path, content)
      return { written: true, path }
    },
  },
}

command("execute-tool", async ({ tool, params }: { tool: string; params: any }) => {
  const handler = tools[tool]
  if (!handler) {
    throw new Error(`Unknown tool: ${tool}. Available: ${Object.keys(tools).join(", ")}`)
  }
  return handler.execute(params)
})

command("list-tools", async () =>
  Object.entries(tools).map(([name, t]) => ({ name, description: t.description })),
)

app.get("/health", c => c.json({ status: "ok" }))

export default app
