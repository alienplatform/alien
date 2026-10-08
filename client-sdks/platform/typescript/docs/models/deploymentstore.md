# DeploymentStore

The secret store a deployer writes a vault-native secret into.

## Example Usage

```typescript
import { DeploymentStore } from "@alienplatform/platform-api/models";

let value: DeploymentStore = "kubernetes-secret";
```

## Values

```typescript
"aws-parameter-store" | "gcp-secret-manager" | "azure-key-vault" | "kubernetes-secret" | "local-vault"
```
