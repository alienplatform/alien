# SyncReconcileResponseTargetReleaseProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseProfileAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTargetReleaseProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncReconcileResponseTargetReleaseProfileAzureBinding](../models/syncreconcileresponsetargetreleaseprofileazurebinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncReconcileResponseTargetReleaseProfileAzureGrant](../models/syncreconcileresponsetargetreleaseprofileazuregrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |