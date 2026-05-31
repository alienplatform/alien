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
| `description`                                                                                                            | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | Short admin-facing description of why this entry exists.                                                                 |
| `effect`                                                                                                                 | [models.SyncAcquireResponseTargetReleaseProfileEffect](../models/syncacquireresponsetargetreleaseprofileeffect.md)       | :heavy_minus_sign:                                                                                                       | IAM effect. Defaults to Allow.                                                                                           |
| `grant`                                                                                                                  | [models.SyncAcquireResponseTargetReleaseProfileAwGrant](../models/syncacquireresponsetargetreleaseprofileawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |
| `label`                                                                                                                  | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | Stable admin-facing label for this permission entry.                                                                     |