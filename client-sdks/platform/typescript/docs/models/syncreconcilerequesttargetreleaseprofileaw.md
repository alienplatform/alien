# SyncReconcileRequestTargetReleaseProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseProfileAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncReconcileRequestTargetReleaseProfileAwBinding](../models/syncreconcilerequesttargetreleaseprofileawbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `effect`                                                                                                                   | [models.SyncReconcileRequestTargetReleaseProfileEffect](../models/syncreconcilerequesttargetreleaseprofileeffect.md)       | :heavy_minus_sign:                                                                                                         | IAM effect. Defaults to Allow.                                                                                             |
| `grant`                                                                                                                    | [models.SyncReconcileRequestTargetReleaseProfileAwGrant](../models/syncreconcilerequesttargetreleaseprofileawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |