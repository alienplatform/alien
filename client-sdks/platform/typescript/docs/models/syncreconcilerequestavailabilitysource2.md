# SyncReconcileRequestAvailabilitySource2

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SyncReconcileRequestAvailabilitySource2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailabilitySource2 = "azure-foundry";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```