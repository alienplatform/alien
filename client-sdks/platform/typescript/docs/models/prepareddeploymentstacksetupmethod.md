# PreparedDeploymentStackSetupMethod

Setup methods that can collect deployer-provided input values.

## Example Usage

```typescript
import { PreparedDeploymentStackSetupMethod } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackSetupMethod = "google-oauth";
```

## Values

```typescript
"cli" | "terraform" | "cloud-formation" | "helm" | "google-oauth"
```