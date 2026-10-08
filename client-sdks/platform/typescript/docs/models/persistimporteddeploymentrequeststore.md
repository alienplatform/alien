# PersistImportedDeploymentRequestStore

The secret store a deployer writes a vault-native secret into.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestStore } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestStore = "aws-parameter-store";
```

## Values

```typescript
"aws-parameter-store" | "gcp-secret-manager" | "azure-key-vault" | "kubernetes-secret" | "local-vault"
```
