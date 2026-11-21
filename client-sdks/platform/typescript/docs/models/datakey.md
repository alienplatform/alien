# DataKey

## Example Usage

```typescript
import { DataKey } from "@alienplatform/platform-api/models";

let value: DataKey = {
  data: {
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
  },
  resourceType: "key",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `data`                                                 | *models.SyncReconcileRequestDataUnion18*               | :heavy_check_mark:                                     | N/A                                                    |
| `resourceType`                                         | [models.ResourceTypeKey](../models/resourcetypekey.md) | :heavy_check_mark:                                     | N/A                                                    |