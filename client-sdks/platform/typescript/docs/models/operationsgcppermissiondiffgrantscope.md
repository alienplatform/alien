# OperationsGcpPermissionDiffGrantScope

Where setup grants the permission: the configured project, or each Cloud Storage bucket the installer lists.

## Example Usage

```typescript
import { OperationsGcpPermissionDiffGrantScope } from "@alienplatform/platform-api/models";

let value: OperationsGcpPermissionDiffGrantScope =
  "projects/${projectName}/buckets/${resourceName}";
```

## Values

```typescript
"projects/${projectName}" | "projects/${projectName}/buckets/${resourceName}"
```