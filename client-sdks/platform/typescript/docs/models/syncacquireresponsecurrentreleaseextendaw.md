# SyncAcquireResponseCurrentReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseExtendAw } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncAcquireResponseCurrentReleaseExtendAwBinding](../models/syncacquireresponsecurrentreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.SyncAcquireResponseCurrentReleaseExtendAwGrant](../models/syncacquireresponsecurrentreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |