# DeploymentRole

Role for deployment-scoped service accounts

## Example Usage

```typescript
import { DeploymentRole } from "@alienplatform/platform-api/models";

let value: DeploymentRole = "deployment.child-deployer";
```

## Values

```typescript
"deployment.viewer" | "deployment.manager" | "deployment.telemetry-writer" | "deployment.child-deployer"
```