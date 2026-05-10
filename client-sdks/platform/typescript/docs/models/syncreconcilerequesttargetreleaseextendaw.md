# SyncReconcileRequestTargetReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseExtendAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncReconcileRequestTargetReleaseExtendAwBinding](../models/syncreconcilerequesttargetreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `effect`                                                                                                                 | [models.SyncReconcileRequestTargetReleaseExtendEffect](../models/syncreconcilerequesttargetreleaseextendeffect.md)       | :heavy_minus_sign:                                                                                                       | IAM effect. Defaults to Allow.                                                                                           |
| `grant`                                                                                                                  | [models.SyncReconcileRequestTargetReleaseExtendAwGrant](../models/syncreconcilerequesttargetreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |