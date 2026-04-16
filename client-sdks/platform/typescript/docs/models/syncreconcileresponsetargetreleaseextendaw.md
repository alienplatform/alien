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
| `grant`                                                                                                                    | [models.SyncReconcileResponseTargetReleaseExtendAwGrant](../models/syncreconcileresponsetargetreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |