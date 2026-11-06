# DeploymentMapStatus

Its status, as the deployment page reads it; mapState groups statuses for the map

## Example Usage

```typescript
import { DeploymentMapStatus } from "@alienplatform/platform-api/models";

let value: DeploymentMapStatus = "deleted";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "waiting-for-machines" | "waiting-for-secrets" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "retention-required" | "retained" | "deleted" | "error"
```