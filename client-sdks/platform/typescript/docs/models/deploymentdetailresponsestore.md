# DeploymentDetailResponseStore

The secret store a deployer writes a vault-native secret into.

## Example Usage

```typescript
import { DeploymentDetailResponseStore } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseStore = "azure-key-vault";
```

## Values

```typescript
"aws-parameter-store" | "gcp-secret-manager" | "azure-key-vault" | "kubernetes-secret" | "local-vault"
```
