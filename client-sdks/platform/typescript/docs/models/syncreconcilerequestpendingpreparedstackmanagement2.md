# SyncReconcileRequestPendingPreparedStackManagement2

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackManagement2 = {
  override: {
    "key": [
      "<value>",
    ],
    "key1": [
      "<value>",
    ],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncReconcileRequestPendingPreparedStackOverrideUnion*[]>                                                  | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
