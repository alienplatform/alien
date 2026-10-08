# SyncListResponseRuntimeMetadataStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { SyncListResponseRuntimeMetadataStatus } from "@alienplatform/platform-api/models";

let value: SyncListResponseRuntimeMetadataStatus = "invalid";
```

## Values

```typescript
"present" | "missing" | "invalid"
```
