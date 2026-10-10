# PermissionsStatus

Whether the Kubernetes RBAC and cloud IAM recorded for this installation match the permissions compiled from every built-in operation plugin, which setup grants to an Operator installed without a release.

## Example Usage

```typescript
import { PermissionsStatus } from "@alienplatform/platform-api/models/operations";

let value: PermissionsStatus = "outdated";
```

## Values

```typescript
"current" | "outdated" | "unknown"
```
