# AiAvailabilitySource

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { AiAvailabilitySource } from "@alienplatform/manager-api/models";

let value: AiAvailabilitySource = "aws-bedrock";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```