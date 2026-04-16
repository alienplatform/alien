# SyncAcquireResponseTargetReleaseOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseOverride } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseOverride = {
  description:
    "between really eek next carefully suspension underneath vet interviewer save",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                              | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Human-readable description of what this permission set allows                                                              |
| `id`                                                                                                                       | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Unique identifier for the permission set (e.g., "storage/data-read")                                                       |
| `platforms`                                                                                                                | [models.SyncAcquireResponseTargetReleaseOverridePlatforms](../models/syncacquireresponsetargetreleaseoverrideplatforms.md) | :heavy_check_mark:                                                                                                         | Platform-specific permission configurations                                                                                |