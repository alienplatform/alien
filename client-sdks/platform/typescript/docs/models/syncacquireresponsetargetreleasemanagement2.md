# SyncAcquireResponseTargetReleaseManagement2

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseManagement2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleaseManagement2 = {
  override: {
    "key": [
      "<value>",
    ],
    "key1": [
      {
        description:
          "safeguard geez anenst how indeed pish account miserly unto",
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
| `override`                                                                                                                        | Record<string, *models.SyncAcquireResponseTargetReleaseOverrideUnion*[]>                                                          | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |