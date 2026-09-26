# PermissionsStatus

Whether the Kubernetes RBAC and cloud IAM recorded for this installation match the permissions compiled from the currently enabled operations.

## Example Usage

```typescript
import { PermissionsStatus } from "@alienplatform/platform-api/models/operations";

let value: PermissionsStatus = "outdated";
```

## Values

```typescript
"current" | "outdated" | "unknown"
```