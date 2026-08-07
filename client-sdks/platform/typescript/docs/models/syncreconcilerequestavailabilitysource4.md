# SyncReconcileRequestAvailabilitySource4

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SyncReconcileRequestAvailabilitySource4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailabilitySource4 = "anthropic";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```