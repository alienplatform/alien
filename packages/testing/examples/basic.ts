/**
 * Basic example of using @alienplatform/testing
 *
 * This demonstrates the simplest use case: deploy an app locally, test it, tear it down.
 */

import { deploy } from "@alienplatform/testing"

async function main() {
  console.log("Deploying test application...")

  // Local mode (default) — no credentials needed
  const deployment = await deploy({
    app: "./fixtures/hello-world",
    verbose: true,
  })

  console.log(`Deployed! URL: ${deployment.url}`)
  console.log(`Deployment ID: ${deployment.id}`)

  // Test the deployment
  try {
    const response = await fetch(`${deployment.url}/api/hello`)
    const data = await response.json()

    console.log(`Response status: ${response.status}`)
    console.log("Response data:", data)

    if (response.status !== 200) {
      throw new Error(`Expected 200, got ${response.status}`)
    }

    console.log("Test passed!")
  } catch (error) {
    console.error("Test failed:", error)
    throw error
  } finally {
    // Always cleanup
    console.log("Cleaning up...")
    await deployment.destroy()
    console.log("Cleanup complete")
  }
}

main().catch(error => {
  console.error("Fatal error:", error)
  process.exit(1)
})
