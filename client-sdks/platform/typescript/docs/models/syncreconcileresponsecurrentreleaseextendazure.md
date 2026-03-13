# SyncReconcileResponseCurrentReleaseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncReconcileResponseCurrentReleaseExtendAzureBinding](../models/syncreconcileresponsecurrentreleaseextendazurebinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncReconcileResponseCurrentReleaseExtendAzureGrant](../models/syncreconcileresponsecurrentreleaseextendazuregrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |