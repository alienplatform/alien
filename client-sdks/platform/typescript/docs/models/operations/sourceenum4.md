# SourceEnum4

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { SourceEnum4 } from "@alienplatform/platform-api/models/operations";

let value: SourceEnum4 = "aws-bedrock";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```