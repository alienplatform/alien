# DeploymentPreparedStackSetupMethod

Setup methods that can collect deployer-provided input values.

## Example Usage

```typescript
import { DeploymentPreparedStackSetupMethod } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackSetupMethod = "terraform";
```

## Values

```typescript
"cli" | "terraform" | "cloud-formation" | "helm" | "google-oauth"
```