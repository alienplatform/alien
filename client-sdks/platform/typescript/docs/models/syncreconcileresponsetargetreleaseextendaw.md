# SyncReconcileResponseTargetReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseExtendAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncReconcileResponseTargetReleaseExtendAwBinding](../models/syncreconcileresponsetargetreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `effect`                                                                                                                   | [models.SyncReconcileResponseTargetReleaseExtendEffect](../models/syncreconcileresponsetargetreleaseextendeffect.md)       | :heavy_minus_sign:                                                                                                         | IAM effect. Defaults to Allow.                                                                                             |
| `grant`                                                                                                                    | [models.SyncReconcileResponseTargetReleaseExtendAwGrant](../models/syncreconcileresponsetargetreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |