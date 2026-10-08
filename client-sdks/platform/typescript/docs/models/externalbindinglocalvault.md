# ExternalBindingLocalVault

Local development vault binding (for testing/development)

## Example Usage

```typescript
import { ExternalBindingLocalVault } from "@alienplatform/platform-api/models";

let value: ExternalBindingLocalVault = {
  dataDir: "<value>",
  vaultName: "<value>",
  service: "local-vault",
  type: "vault",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `vaultName`                                                                                                          | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | The vault name for local storage                                                                                     |
| `service`                                                                                                            | *"local-vault"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeVault5](../models/typevault5.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
