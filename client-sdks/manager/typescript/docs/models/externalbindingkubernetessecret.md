# ExternalBindingKubernetesSecret

Kubernetes Secrets binding (native K8s secret storage)

## Example Usage

```typescript
import { ExternalBindingKubernetesSecret } from "@alienplatform/manager-api/models";

let value: ExternalBindingKubernetesSecret = {
  namespace: "<value>",
  vaultPrefix: "<value>",
  service: "kubernetes-secret",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultPrefix`                                                                                                        | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"kubernetes-secret"*                                                                                                | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeVault4](../models/typevault4.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |