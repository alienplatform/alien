# DeploymentConfigExternalBindingsParameterStore

AWS SSM Parameter Store vault binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsParameterStore } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *models.DeploymentConfigVaultPrefixUnion1*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"parameter-store"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeVault1](../models/deploymentconfigtypevault1.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |