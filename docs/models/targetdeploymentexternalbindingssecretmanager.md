# TargetDeploymentExternalBindingsSecretManager

GCP Secret Manager vault binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsSecretManager } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsSecretManager = {
  service: "secret-manager",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *models.TargetDeploymentVaultPrefixUnion2*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"secret-manager"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeVault2](../models/targetdeploymenttypevault2.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |