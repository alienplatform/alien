# TargetDeploymentExternalBindingsParameterStore

AWS SSM Parameter Store vault binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsParameterStore } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsParameterStore = {
  service: "parameter-store",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *models.TargetDeploymentVaultPrefixUnion1*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"parameter-store"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeVault1](../models/targetdeploymenttypevault1.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |