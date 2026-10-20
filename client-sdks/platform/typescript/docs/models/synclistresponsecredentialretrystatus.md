# SyncListResponseCredentialRetryStatus

Changing phase starts a new window instead of carrying a previous outage forward.

## Example Usage

```typescript
import { SyncListResponseCredentialRetryStatus } from "@alienplatform/platform-api/models";

let value: SyncListResponseCredentialRetryStatus = "preflights-failed";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "waiting-for-machines" | "waiting-for-secrets" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "deleted" | "error"
```