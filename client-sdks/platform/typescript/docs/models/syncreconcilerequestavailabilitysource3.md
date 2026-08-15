# SyncReconcileRequestAvailabilitySource3

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SyncReconcileRequestAvailabilitySource3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailabilitySource3 = "gcp-vertex";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```