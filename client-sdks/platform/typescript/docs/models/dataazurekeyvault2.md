# DataAzureKeyVault2

## Example Usage

```typescript
import { DataAzureKeyVault2 } from "@alienplatform/platform-api/models";

let value: DataAzureKeyVault2 = {
  data: {
    keyId: "<id>",
    keyOperations: [],
    keyType: "<value>",
    status: {
      health: "unknown",
      lifecycle: "deleting",
    },
  },
  provider: "azure-key-vault",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData8](../models/syncreconcilerequestdata8.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `provider`                                                                 | *"azure-key-vault"*                                                        | :heavy_check_mark:                                                         | N/A                                                                        |