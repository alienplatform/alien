# SyncReconcileRequestCurrentReleaseManagement1

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseManagement1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseManagement1 = {
  extend: {
    "key": [],
    "key1": [
      {
        description:
          "deed heavenly lazily anaesthetise besides poorly repossess why skean",
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
| `extend`                                                                                                                          | Record<string, *models.SyncReconcileRequestCurrentReleaseExtendUnion*[]>                                                          | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |