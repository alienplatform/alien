# ExternalBindingSecretManager

GCP Secret Manager vault binding configuration

## Example Usage

```typescript
import { ExternalBindingSecretManager } from "@alienplatform/platform-api/models";

let value: ExternalBindingSecretManager = {
  vaultPrefix: "<value>",
  service: "secret-manager",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"secret-manager"*                                                                                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeVault2](../models/typevault2.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
