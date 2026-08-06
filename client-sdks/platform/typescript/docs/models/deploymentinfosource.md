# DeploymentInfoSource

Provider control plane used to observe model availability without invoking
a model, spending customer quota, or accepting provider terms.

## Example Usage

```typescript
import { DeploymentInfoSource } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSource = "aws-bedrock";
```

## Values

```typescript
"aws-bedrock" | "gcp-vertex" | "azure-foundry" | "anthropic"
```