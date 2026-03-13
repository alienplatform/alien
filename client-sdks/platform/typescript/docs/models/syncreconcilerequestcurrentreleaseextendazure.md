# SyncReconcileRequestCurrentReleaseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseExtendAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncReconcileRequestCurrentReleaseExtendAzureBinding](../models/syncreconcilerequestcurrentreleaseextendazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncReconcileRequestCurrentReleaseExtendAzureGrant](../models/syncreconcilerequestcurrentreleaseextendazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |