# DeploymentAcquireUnavailableReason

Why an explicitly requested deployment was not acquired.

These reasons are intentionally bounded and do not identify the session
that owns a competing lease.

## Example Usage

```typescript
import { DeploymentAcquireUnavailableReason } from "@alienplatform/manager-api/models";

let value: DeploymentAcquireUnavailableReason = "acquireModeMismatch";
```

## Values

```typescript
"contended" | "deferred" | "statusMismatch" | "deploymentModelMismatch" | "platformMismatch" | "setupMethodMismatch" | "acquireModeMismatch" | "limitReached"
```