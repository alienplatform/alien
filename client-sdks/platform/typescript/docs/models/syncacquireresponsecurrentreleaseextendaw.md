# SyncAcquireResponseCurrentReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseExtendAw } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncAcquireResponseCurrentReleaseExtendAwBinding](../models/syncacquireresponsecurrentreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `description`                                                                                                            | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | Short admin-facing description of why this entry exists.                                                                 |
| `effect`                                                                                                                 | [models.SyncAcquireResponseCurrentReleaseExtendEffect](../models/syncacquireresponsecurrentreleaseextendeffect.md)       | :heavy_minus_sign:                                                                                                       | IAM effect. Defaults to Allow.                                                                                           |
| `grant`                                                                                                                  | [models.SyncAcquireResponseCurrentReleaseExtendAwGrant](../models/syncacquireresponsecurrentreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |
| `label`                                                                                                                  | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | Stable admin-facing label for this permission entry.                                                                     |