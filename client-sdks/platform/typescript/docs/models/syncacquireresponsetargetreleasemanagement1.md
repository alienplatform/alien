# SyncAcquireResponseTargetReleaseManagement1

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseManagement1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleaseManagement1 = {
  extend: {
    "key": [],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncAcquireResponseTargetReleaseExtendUnion*[]>                                                            | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |