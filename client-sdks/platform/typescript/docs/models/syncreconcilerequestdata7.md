# SyncReconcileRequestData7

## Example Usage

```typescript
import { SyncReconcileRequestData7 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData7 = {
  cryptoKeyName: "<value>",
  purpose: "<value>",
  status: {
    health: "degraded",
    lifecycle: "running",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `algorithm`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `cryptoKeyName`                                                                  | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `primaryState`                                                                   | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `primaryVersion`                                                                 | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `purpose`                                                                        | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.SyncReconcileRequestStatus71](../models/syncreconcilerequeststatus71.md) | :heavy_check_mark:                                                               | N/A                                                                              |