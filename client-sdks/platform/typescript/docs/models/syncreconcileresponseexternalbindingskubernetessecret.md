# SyncReconcileResponseExternalBindingsKubernetesSecret

Kubernetes Secrets vault binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsKubernetesSecret } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsKubernetesSecret = {
  service: "kubernetes-secret",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultPrefix`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"kubernetes-secret"*                                                                                                | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeVault4](../models/syncreconcileresponsetypevault4.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |