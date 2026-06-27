# SyncReconcileResponseTargetReleaseSetupMethod

Setup methods that can collect deployer-provided input values.

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseSetupMethod } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseSetupMethod = "helm";
```

## Values

```typescript
"cli" | "terraform" | "cloud-formation" | "helm" | "google-oauth"
```