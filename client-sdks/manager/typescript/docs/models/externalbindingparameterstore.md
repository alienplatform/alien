# ExternalBindingParameterStore

AWS SSM Parameter Store binding (SecureString)

## Example Usage

```typescript
import { ExternalBindingParameterStore } from "@alienplatform/manager-api/models";

let value: ExternalBindingParameterStore = {
  vaultPrefix: "<value>",
  service: "parameter-store",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"parameter-store"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeVault1](../models/typevault1.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |