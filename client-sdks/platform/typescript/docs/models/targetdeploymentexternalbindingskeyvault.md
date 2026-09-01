# TargetDeploymentExternalBindingsKeyVault

Azure Key Vault binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsKeyVault } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsKeyVault = {
  service: "key-vault",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultName`                                                                                                          | *models.TargetDeploymentVaultNameUnion*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"key-vault"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeVault3](../models/targetdeploymenttypevault3.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |