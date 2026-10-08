# ExternalBindingParameterStore

AWS SSM Parameter Store vault binding configuration

## Example Usage

```typescript
import { ExternalBindingParameterStore } from "@alienplatform/platform-api/models";

let value: ExternalBindingParameterStore = {
  vaultPrefix: "<value>",
  service: "parameter-store",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `vaultPrefix`                                                                                                        | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"parameter-store"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeVault1](../models/typevault1.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |