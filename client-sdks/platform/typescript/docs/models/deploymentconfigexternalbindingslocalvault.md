# DeploymentConfigExternalBindingsLocalVault

Local development vault binding (for testing/development)

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsLocalVault } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.DeploymentConfigDataDirUnion2*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultName`                                                                                                          | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | The vault name for local storage                                                                                     |
| `service`                                                                                                            | *"local-vault"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeVault5](../models/deploymentconfigtypevault5.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |