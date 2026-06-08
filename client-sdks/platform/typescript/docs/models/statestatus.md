# StateStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { StateStatus } from "@alienplatform/platform-api/models";

let value: StateStatus = "running";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "deleted" | "error"
```