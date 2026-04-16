# SyncReconcileRequestTargetReleaseManagement1

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseManagement1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseManagement1 = {
  extend: {
    "key": [
      "<value>",
    ],
    "key1": [],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncReconcileRequestTargetReleaseExtendUnion*[]>                                                           | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |