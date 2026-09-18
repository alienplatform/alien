# SourceEnum2

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SourceEnum2 } from "@alienplatform/platform-api/models/operations";

let value: SourceEnum2 = "azure-foundry";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```