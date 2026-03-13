# SyncAcquireResponseCurrentReleaseManagement2

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseManagement2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseManagement2 = {
  override: {
    "key": [
      {
        description: "internal cleave hmph rewrite uh-huh sizzle meanwhile eek",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncAcquireResponseCurrentReleaseOverrideUnion*[]>                                                         | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |