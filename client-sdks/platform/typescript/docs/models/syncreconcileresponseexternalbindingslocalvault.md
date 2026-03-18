# SyncReconcileResponseExternalBindingsLocalVault

Local development vault binding (for testing/development)

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsLocalVault } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsLocalVault = {
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *models.SyncReconcileResponseDataDirUnion2*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultName`                                                                                                          | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | The vault name for local storage                                                                                     |
| `service`                                                                                                            | *"local-vault"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeVault5](../models/syncreconcileresponsetypevault5.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |