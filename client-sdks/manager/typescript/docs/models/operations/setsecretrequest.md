# SetSecretRequest

## Example Usage

```typescript
import { SetSecretRequest } from "@alienplatform/manager-api/models/operations";

let value: SetSecretRequest = {
  id: "<id>",
  vaultName: "<value>",
  key: "<key>",
  setSecretRequest: {
    value: "<value>",
  },
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `id`                                                        | *string*                                                    | :heavy_check_mark:                                          | Deployment ID                                               |
| `vaultName`                                                 | *string*                                                    | :heavy_check_mark:                                          | Vault name                                                  |
| `key`                                                       | *string*                                                    | :heavy_check_mark:                                          | Secret name                                                 |
| `setSecretRequest`                                          | [models.SetSecretRequest](../../models/setsecretrequest.md) | :heavy_check_mark:                                          | N/A                                                         |
