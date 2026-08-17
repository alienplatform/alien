# DeploymentConfigExternalBindingsSecretManager

GCP Secret Manager vault binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsSecretManager } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *models.DeploymentConfigVaultPrefixUnion2*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"secret-manager"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeVault2](../models/deploymentconfigtypevault2.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |