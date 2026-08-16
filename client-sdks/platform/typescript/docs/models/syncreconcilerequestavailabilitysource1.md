# SyncReconcileRequestAvailabilitySource1

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SyncReconcileRequestAvailabilitySource1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailabilitySource1 = "gcp-vertex";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```