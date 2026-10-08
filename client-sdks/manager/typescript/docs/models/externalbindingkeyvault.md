# ExternalBindingKeyVault

Azure Key Vault binding

## Example Usage

```typescript
import { ExternalBindingKeyVault } from "@alienplatform/manager-api/models";

let value: ExternalBindingKeyVault = {
  vaultName: "<value>",
  service: "key-vault",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultName`                                                                                                          | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"key-vault"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeVault3](../models/typevault3.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |