# SyncAcquireResponseCurrentReleaseProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseProfileAw } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncAcquireResponseCurrentReleaseProfileAwBinding](../models/syncacquireresponsecurrentreleaseprofileawbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `description`                                                                                                              | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | Short admin-facing description of why this entry exists.                                                                   |
| `effect`                                                                                                                   | [models.SyncAcquireResponseCurrentReleaseProfileEffect](../models/syncacquireresponsecurrentreleaseprofileeffect.md)       | :heavy_minus_sign:                                                                                                         | IAM effect. Defaults to Allow.                                                                                             |
| `grant`                                                                                                                    | [models.SyncAcquireResponseCurrentReleaseProfileAwGrant](../models/syncacquireresponsecurrentreleaseprofileawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |
| `label`                                                                                                                    | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | Stable admin-facing label for this permission entry.                                                                       |