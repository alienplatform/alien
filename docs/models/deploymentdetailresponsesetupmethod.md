# DeploymentDetailResponseSetupMethod

Setup method that created the deployment record and owns setup-time resources.

## Example Usage

```typescript
import { DeploymentDetailResponseSetupMethod } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseSetupMethod = "cli";
```

## Values

```typescript
"cloudformation" | "google-oauth" | "terraform" | "helm" | "cli" | "manual"
```