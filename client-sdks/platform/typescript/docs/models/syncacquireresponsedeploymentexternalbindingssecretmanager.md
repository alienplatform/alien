# SyncAcquireResponseDeploymentExternalBindingsSecretManager

GCP Secret Manager vault binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsSecretManager } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *models.SyncAcquireResponseDeploymentVaultPrefixUnion2*                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"secret-manager"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeVault2](../models/syncacquireresponsedeploymenttypevault2.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |