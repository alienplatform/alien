# DeploymentDetailResponseRuntimeMetadataStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { DeploymentDetailResponseRuntimeMetadataStatus } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseRuntimeMetadataStatus = "present";
```

## Values

```typescript
"present" | "missing" | "invalid"
```
