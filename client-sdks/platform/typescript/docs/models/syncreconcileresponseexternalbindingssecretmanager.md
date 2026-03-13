# SyncReconcileResponseExternalBindingsSecretManager

GCP Secret Manager vault binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsSecretManager } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"secret-manager"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeVault2](../models/syncreconcileresponsetypevault2.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |