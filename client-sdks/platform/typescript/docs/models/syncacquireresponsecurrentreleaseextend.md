# SyncAcquireResponseCurrentReleaseExtend

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseExtend } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseExtend = {
  description:
    "who furthermore disarm pastel celebrated why boohoo liquid midst browse",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `description`                                                                                                            | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Human-readable description of what this permission set allows                                                            |
| `id`                                                                                                                     | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Unique identifier for the permission set (e.g., "storage/data-read")                                                     |
| `platforms`                                                                                                              | [models.SyncAcquireResponseCurrentReleaseExtendPlatforms](../models/syncacquireresponsecurrentreleaseextendplatforms.md) | :heavy_check_mark:                                                                                                       | Platform-specific permission configurations                                                                              |