# PersistImportedDeploymentRequestSetupMethod

Setup methods that can collect deployer-provided input values.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestSetupMethod } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestSetupMethod = "cli";
```

## Values

```typescript
"cli" | "terraform" | "cloud-formation" | "helm" | "google-oauth"
```