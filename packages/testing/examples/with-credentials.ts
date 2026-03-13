/**
 * Example using explicit credentials
 *
 * This shows how to pass platform credentials explicitly instead of using environment variables.
 */

import { deploy } from "@aliendotdev/testing"

async function main() {
  console.log("Deploying to AWS with explicit credentials...")

  const deployment = await deploy({
    app: "./fixtures/hello-world",
    platform: "aws",
    credentials: {
      platform: "aws",
      accessKeyId: process.env.AWS_ACCESS_KEY_ID!,
      secretAccessKey: process.env.AWS_SECRET_ACCESS_KEY!,
      region: "us-east-1",
    },
    environmentVariables: [
      {
        name: "DATABASE_URL",
        value: "postgres://localhost/mydb",
        type: "plain",
        targetResources: ["*"],
      },
      {
        name: "API_KEY",
        value: "secret-key",
        type: "secret",
        targetResources: ["*"],
      },
    ],
    verbose: true,
  })

  console.log(`Deployed! URL: ${deployment.url}`)

  try {
    // Test the deployment
    const response = await fetch(`${deployment.url}/api/test`)
    console.log(`Status: ${response.status}`)

    if (response.status !== 200) {
      throw new Error(`Expected 200, got ${response.status}`)
    }

    console.log("✅ All tests passed!")
  } finally {
    await deployment.destroy()
  }
}

main().catch(error => {
  console.error("Error:", error)
  process.exit(1)
})
