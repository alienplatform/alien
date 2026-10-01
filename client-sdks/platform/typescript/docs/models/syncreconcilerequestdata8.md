# SyncReconcileRequestData8

## Example Usage

```typescript
import { SyncReconcileRequestData8 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData8 = {
  keyId: "<id>",
  keyOperations: [],
  keyType: "<value>",
  status: {
    health: "unknown",
    lifecycle: "deleting",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `enabled`                                                                        | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `keyId`                                                                          | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `keyOperations`                                                                  | *string*[]                                                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `keyType`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `recoveryLevel`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.SyncReconcileRequestStatus72](../models/syncreconcilerequeststatus72.md) | :heavy_check_mark:                                                               | N/A                                                                              |