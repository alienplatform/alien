# SyncReconcileRequestTargetReleaseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseOverrideAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncReconcileRequestTargetReleaseOverrideAzureBinding](../models/syncreconcilerequesttargetreleaseoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncReconcileRequestTargetReleaseOverrideAzureGrant](../models/syncreconcilerequesttargetreleaseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |