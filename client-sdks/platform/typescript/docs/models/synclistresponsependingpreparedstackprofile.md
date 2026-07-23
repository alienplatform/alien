# SyncListResponsePendingPreparedStackProfile

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackProfile } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackProfile = {
  description:
    "whereas spectacles er late seldom absentmindedly experienced where",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                    | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | Human-readable description of what this permission set allows                                                                    |
| `id`                                                                                                                             | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | Unique identifier for the permission set (e.g., "storage/data-read")                                                             |
| `platforms`                                                                                                                      | [models.SyncListResponsePendingPreparedStackProfilePlatforms](../models/synclistresponsependingpreparedstackprofileplatforms.md) | :heavy_check_mark:                                                                                                               | Platform-specific permission configurations                                                                                      |
