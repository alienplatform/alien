# DataKey

## Example Usage

```typescript
import { DataKey } from "@alienplatform/platform-api/models/operations";

let value: DataKey = {
  data: {
    data: {
      keyId: "<id>",
      keyOperations: [],
      keyType: "<value>",
      status: {
        health: "unknown",
        lifecycle: "running",
      },
    },
    provider: "azure-key-vault",
  },
  resourceType: "key",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `data`                                                                   | *operations.DataUnion18*                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `resourceType`                                                           | [operations.ResourceTypeKey](../../models/operations/resourcetypekey.md) | :heavy_check_mark:                                                       | N/A                                                                      |