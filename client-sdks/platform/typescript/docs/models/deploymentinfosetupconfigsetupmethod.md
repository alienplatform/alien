# DeploymentInfoSetupConfigSetupMethod

Setup methods that can collect deployer-provided input values.

## Example Usage

```typescript
import { DeploymentInfoSetupConfigSetupMethod } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSetupConfigSetupMethod = "terraform";
```

## Values

```typescript
"cli" | "terraform" | "cloud-formation" | "helm" | "google-oauth"
```