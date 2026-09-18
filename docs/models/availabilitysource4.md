# AvailabilitySource4

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { AvailabilitySource4 } from "@alienplatform/platform-api/models";

let value: AvailabilitySource4 = "gcp-vertex";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```