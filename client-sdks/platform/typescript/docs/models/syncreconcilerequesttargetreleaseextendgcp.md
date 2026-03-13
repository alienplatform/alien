# SyncReconcileRequestTargetReleaseExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseExtendGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncReconcileRequestTargetReleaseExtendGcpBinding](../models/syncreconcilerequesttargetreleaseextendgcpbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `grant`                                                                                                                    | [models.SyncReconcileRequestTargetReleaseExtendGcpGrant](../models/syncreconcilerequesttargetreleaseextendgcpgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |