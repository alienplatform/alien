# SyncReconcileResponseTargetReleaseManagement1

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseManagement1 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTargetReleaseManagement1 = {
  extend: {
    "key": [],
    "key1": [
      {
        description:
          "swathe ghost promptly within psst ouch dismal wrong however",
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
| `extend`                                                                                                                          | Record<string, *models.SyncReconcileResponseTargetReleaseExtendUnion*[]>                                                          | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |