# PersistImportedDeploymentRequestRuntimeMetadataStatus

Whether a deployer secret slot holds a usable value. Alien learns this from
metadata only and never reads the value.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestRuntimeMetadataStatus } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestRuntimeMetadataStatus = "missing";
```

## Values

```typescript
"present" | "missing" | "invalid"
```