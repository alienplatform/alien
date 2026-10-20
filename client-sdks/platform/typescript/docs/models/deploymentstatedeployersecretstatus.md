# DeploymentStateDeployerSecretStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { DeploymentStateDeployerSecretStatus } from "@alienplatform/platform-api/models";

let value: DeploymentStateDeployerSecretStatus = "missing";
```

## Values

```typescript
"present" | "missing" | "invalid"
```