# SyncAcquireResponseDeploymentExternalBindingsParameterStore

AWS SSM Parameter Store vault binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsParameterStore } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *models.SyncAcquireResponseDeploymentVaultPrefixUnion1*                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"parameter-store"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeVault1](../models/syncacquireresponsedeploymenttypevault1.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |