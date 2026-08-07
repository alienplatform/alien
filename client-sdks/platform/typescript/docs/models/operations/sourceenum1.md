# SourceEnum1

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SourceEnum1 } from "@alienplatform/platform-api/models/operations";

let value: SourceEnum1 = "anthropic";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```