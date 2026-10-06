# DeploymentStateRuntimeMetadataStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { DeploymentStateRuntimeMetadataStatus } from "@alienplatform/platform-api/models";

let value: DeploymentStateRuntimeMetadataStatus = "invalid";
```

## Values

```typescript
"present" | "missing" | "invalid"
```