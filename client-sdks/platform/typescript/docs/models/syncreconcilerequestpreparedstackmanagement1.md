# SyncReconcileRequestPreparedStackManagement1

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackManagement1 = {
  extend: {},
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncReconcileRequestPreparedStackExtendUnion*[]>                                                           | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |