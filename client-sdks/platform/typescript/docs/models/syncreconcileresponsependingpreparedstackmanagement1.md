# SyncReconcileResponsePendingPreparedStackManagement1

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackManagement1 = {
  extend: {
    "key": [],
    "key1": [
      {
        description: "now forager phooey cinder finally whenever restfully ack",
        id: "<id>",
        platforms: {},
      },
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncReconcileResponsePendingPreparedStackExtendUnion*[]>                                                   | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
