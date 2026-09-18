# SyncListResponseSetupMethod

Setup method that created the deployment record and owns setup-time resources.

## Example Usage

```typescript
import { SyncListResponseSetupMethod } from "@alienplatform/platform-api/models";

let value: SyncListResponseSetupMethod = "google-oauth";
```

## Values

```typescript
"cloudformation" | "google-oauth" | "terraform" | "helm" | "cli" | "manual"
```