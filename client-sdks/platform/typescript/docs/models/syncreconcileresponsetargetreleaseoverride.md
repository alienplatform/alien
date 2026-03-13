# SyncReconcileResponseTargetReleaseOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseOverride } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTargetReleaseOverride = {
  description:
    "acknowledge miserably geez boohoo likewise yippee furthermore hence translation meh",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `description`                                                                                                                  | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | Human-readable description of what this permission set allows                                                                  |
| `id`                                                                                                                           | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | Unique identifier for the permission set (e.g., "storage/data-read")                                                           |
| `platforms`                                                                                                                    | [models.SyncReconcileResponseTargetReleaseOverridePlatforms](../models/syncreconcileresponsetargetreleaseoverrideplatforms.md) | :heavy_check_mark:                                                                                                             | Platform-specific permission configurations                                                                                    |