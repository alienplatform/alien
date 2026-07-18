# SyncAcquireResponseDeploymentExternalBindingsKeyVault

Azure Key Vault binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsKeyVault } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsKeyVault = {
  service: "key-vault",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultName`                                                                                                          | *models.SyncAcquireResponseDeploymentVaultNameUnion*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"key-vault"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeVault3](../models/syncacquireresponsedeploymenttypevault3.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |