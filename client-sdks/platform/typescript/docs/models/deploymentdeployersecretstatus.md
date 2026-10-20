# DeploymentDeployerSecretStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { DeploymentDeployerSecretStatus } from "@alienplatform/platform-api/models";

let value: DeploymentDeployerSecretStatus = "present";
```

## Values

```typescript
"present" | "missing" | "invalid"
```