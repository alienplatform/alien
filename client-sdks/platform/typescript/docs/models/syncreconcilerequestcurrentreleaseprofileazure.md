# SyncReconcileRequestCurrentReleaseProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseProfileAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncReconcileRequestCurrentReleaseProfileAzureBinding](../models/syncreconcilerequestcurrentreleaseprofileazurebinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncReconcileRequestCurrentReleaseProfileAzureGrant](../models/syncreconcilerequestcurrentreleaseprofileazuregrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |