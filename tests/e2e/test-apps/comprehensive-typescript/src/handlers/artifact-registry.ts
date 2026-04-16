import { artifactRegistry } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/artifact-registry-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const ar = await artifactRegistry(bindingName)
    const testRepoName = `test-ts-repo-${Date.now()}`

    // 1. Create repository
    const repo = await ar.createRepository(testRepoName)

    // 2. Generate credentials (Pull)
    await ar.generateCredentials(repo.name, "pull", 3600)

    // 3. Generate credentials (PushPull)
    await ar.generateCredentials(repo.name, "push-pull", 3600)

    // 4. Delete repository
    await ar.deleteRepository(repo.name)

    return c.json({ success: true, bindingName, repoName: testRepoName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "artifact-registry-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
