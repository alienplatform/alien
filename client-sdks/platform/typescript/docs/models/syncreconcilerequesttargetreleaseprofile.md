# SyncReconcileRequestTargetReleaseProfile

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseProfile } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseProfile = {
  description:
    "effector truthfully reach whoa import typeface shameful even through",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                              | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Human-readable description of what this permission set allows                                                              |
| `id`                                                                                                                       | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Unique identifier for the permission set (e.g., "storage/data-read")                                                       |
| `platforms`                                                                                                                | [models.SyncReconcileRequestTargetReleaseProfilePlatforms](../models/syncreconcilerequesttargetreleaseprofileplatforms.md) | :heavy_check_mark:                                                                                                         | Platform-specific permission configurations                                                                                |