# SyncAcquireResponseTargetReleaseProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleaseProfileAw } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseTargetReleaseProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncAcquireResponseTargetReleaseProfileAwBinding](../models/syncacquireresponsetargetreleaseprofileawbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.SyncAcquireResponseTargetReleaseProfileAwGrant](../models/syncacquireresponsetargetreleaseprofileawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |