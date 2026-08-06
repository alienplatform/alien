# SourceEnum3

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SourceEnum3 } from "@alienplatform/platform-api/models/operations";

let value: SourceEnum3 = "azure-foundry";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```