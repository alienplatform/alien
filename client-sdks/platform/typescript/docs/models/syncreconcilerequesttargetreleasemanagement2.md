# SyncReconcileRequestTargetReleaseManagement2

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseManagement2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseManagement2 = {
  override: {
    "key": [
      "<value>",
    ],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncReconcileRequestTargetReleaseOverrideUnion*[]>                                                         | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |