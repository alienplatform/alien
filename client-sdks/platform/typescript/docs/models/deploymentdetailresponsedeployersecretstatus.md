# DeploymentDetailResponseDeployerSecretStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { DeploymentDetailResponseDeployerSecretStatus } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseDeployerSecretStatus = "invalid";
```

## Values

```typescript
"present" | "missing" | "invalid"
```