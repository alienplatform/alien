# DeploymentSetupMethod1

Setup method that created the deployment record and owns setup-time resources.

## Example Usage

```typescript
import { DeploymentSetupMethod1 } from "@alienplatform/platform-api/models";

let value: DeploymentSetupMethod1 = "terraform";
```

## Values

```typescript
"cloudformation" | "google-oauth" | "terraform" | "helm" | "cli" | "manual"
```