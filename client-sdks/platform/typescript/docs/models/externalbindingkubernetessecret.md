# ExternalBindingKubernetesSecret

Kubernetes Secrets vault binding configuration

## Example Usage

```typescript
import { ExternalBindingKubernetesSecret } from "@alienplatform/platform-api/models";

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
| `namespace`                                                                                                          | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultPrefix`                                                                                                        | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"kubernetes-secret"*                                                                                                | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeVault4](../models/typevault4.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |