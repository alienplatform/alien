# TargetDeploymentExternalBindingsLocalVault

Local development vault binding (for testing/development)

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsLocalVault } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.TargetDeploymentDataDirUnion2*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultName`                                                                                                          | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | The vault name for local storage                                                                                     |
| `service`                                                                                                            | *"local-vault"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeVault5](../models/configtypevault5.md)                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |