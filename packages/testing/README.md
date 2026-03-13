# @aliendotdev/testing

Testing framework for Alien applications. Deploy, test, and tear down Alien apps in real environments.

## Installation

```bash
npm install @aliendotdev/testing
```

## Quick Start

```typescript
import { deploy } from "@aliendotdev/testing"

const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
})

// Test your deployment
const response = await fetch(`${deployment.url}/api/test`)
expect(response.status).toBe(200)

// Cleanup
await deployment.destroy()
```

## Authentication

### API Key

Set your Alien API key:

```bash
export ALIEN_API_KEY="your-key"
# or
alien login
```

### Platform Credentials

Credentials are **optional**. When not provided, deployers use standard environment variables.

#### AWS
```bash
export AWS_ACCESS_KEY_ID="..."
export AWS_SECRET_ACCESS_KEY="..."
export AWS_REGION="us-east-1"
```

Or pass explicitly:
```typescript
credentials: {
  platform: "aws",
  accessKeyId: "...",
  secretAccessKey: "...",
  region: "us-east-1",
}
```

#### GCP
```bash
export GOOGLE_APPLICATION_CREDENTIALS="/path/to/key.json"
export GCP_PROJECT_ID="my-project"
```

Or pass explicitly:
```typescript
credentials: {
  platform: "gcp",
  projectId: "my-project",
  serviceAccountKeyPath: "/path/to/key.json",
  // or
  serviceAccountKeyJson: "{ ... }",
}
```

#### Azure
```bash
export AZURE_SUBSCRIPTION_ID="..."
export AZURE_TENANT_ID="..."
export AZURE_CLIENT_ID="..."
export AZURE_CLIENT_SECRET="..."
```

Or pass explicitly:
```typescript
credentials: {
  platform: "azure",
  subscriptionId: "...",
  tenantId: "...",
  clientId: "...",
  clientSecret: "...",
}
```

#### Kubernetes
```bash
export KUBECONFIG="~/.kube/config"
```

Or pass explicitly:
```typescript
credentials: {
  platform: "kubernetes",
  kubeconfigPath: "~/.kube/config",
}
```

## Deployment Methods

### API (Default)

Fastest method. Agent Manager deploys directly:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
  method: "api", // default
})
```

### CLI

Tests the actual CLI deployment flow:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
  method: "cli",
})
```

### Terraform

Tests Terraform provider:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
  method: "terraform",
})
```

### CloudFormation

Tests CloudFormation deployment (AWS only):

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
  method: "cloudformation",
})
```

### Helm

Tests Helm chart deployment (Kubernetes only):

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "kubernetes",
  workspace: "my-workspace",
  project: "my-project",
  method: "helm",
  valuesYaml: "./values.yaml",
  namespace: "default",
})
```

### Operator Image

Tests pull-mode deployment via Docker operator:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
  method: "operator-image",
})
```

## Query Logs

Query deployment logs using DeepStore:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "test",
  workspace: "my-workspace",
  project: "my-project",
})

// Configure log querying
const logsConfig = {
  managerUrl: "http://localhost:3000",
  deepstoreServerUrl: "http://localhost:8080",
  databaseId: "db_123",
  agentToken: "token_123",
}

const logs = await deployment.queryLogs({
  query: "level:ERROR",
  startTime: new Date(Date.now() - 3600_000), // 1 hour ago
  endTime: new Date(),
  maxHits: 100,
})

console.log(`Found ${logs.num_hits} logs`)
```

## Environment Variables

Pass environment variables to your deployment:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
  environmentVariables: [
    { name: "DATABASE_URL", value: "postgres://...", type: "plaintext" },
    { name: "API_KEY", value: "secret", type: "secret" },
  ],
})
```

## Stack Settings

Customize deployment behavior:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
  workspace: "my-workspace",
  project: "my-project",
  stackSettings: {
    deploymentModel: "push",
    heartbeats: "on",
    telemetry: "auto",
    updates: "auto",
  },
})
```

## Example Test

```typescript
import { describe, it, expect } from "vitest"
import { deploy } from "@aliendotdev/testing"

describe("my app", () => {
  it("should deploy and respond", async () => {
    const deployment = await deploy({
      app: "./fixtures/my-app",
      platform: "test",
      workspace: "test-workspace",
      project: "test-project",
    })

    try {
      const response = await fetch(`${deployment.url}/api/hello`)
      const data = await response.json()
      
      expect(response.status).toBe(200)
      expect(data.message).toBe("Hello, World!")
    } finally {
      await deployment.destroy()
    }
  }, 180_000) // 3 min timeout
})
```

## API Reference

### `deploy(options: DeployOptions): Promise<Deployment>`

Deploy an Alien application for testing.

### `Deployment`

Handle to a deployed application.

**Properties:**
- `id: string` - Agent ID
- `name: string` - Agent name
- `url: string` - Deployment URL
- `platform: Platform` - Target platform
- `status: AgentStatus` - Current status

**Methods:**
- `refresh(): Promise<void>` - Refresh deployment info from API
- `waitForStatus(status: AgentStatus, options?: WaitOptions): Promise<void>` - Wait for specific status
- `queryLogs(query: LogQuery): Promise<LogQueryResult>` - Query deployment logs
- `destroy(): Promise<void>` - Tear down the deployment

## License

ISC
