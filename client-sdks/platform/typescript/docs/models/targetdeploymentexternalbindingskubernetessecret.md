# TargetDeploymentExternalBindingsKubernetesSecret

Kubernetes Secrets vault binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsKubernetesSecret } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsKubernetesSecret = {
  service: "kubernetes-secret",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.TargetDeploymentNamespaceUnion2*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultPrefix`                                                                                                        | *models.TargetDeploymentVaultPrefixUnion3*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"kubernetes-secret"*                                                                                                | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeVault4](../models/configtypevault4.md)                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |