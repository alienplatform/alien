# SyncListResponseDeployerSecretStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { SyncListResponseDeployerSecretStatus } from "@alienplatform/platform-api/models";

let value: SyncListResponseDeployerSecretStatus = "present";
```

## Values

```typescript
"present" | "missing" | "invalid"
```