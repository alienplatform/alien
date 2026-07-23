# SyncReconcileRequestPendingPreparedStackManagement1

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackManagement1 = {
  extend: {
    "key": [],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncReconcileRequestPendingPreparedStackExtendUnion*[]>                                                    | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
