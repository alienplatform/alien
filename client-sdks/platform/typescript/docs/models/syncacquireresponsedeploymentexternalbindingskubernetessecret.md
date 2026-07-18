# SyncAcquireResponseDeploymentExternalBindingsKubernetesSecret

Kubernetes Secrets vault binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsKubernetesSecret } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsKubernetesSecret = {
  service: "kubernetes-secret",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.SyncAcquireResponseDeploymentNamespaceUnion2*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultPrefix`                                                                                                        | *models.SyncAcquireResponseDeploymentVaultPrefixUnion3*                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"kubernetes-secret"*                                                                                                | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeVault4](../models/syncacquireresponsedeploymenttypevault4.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |