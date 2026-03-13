# SyncReconcileResponseTargetReleaseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncReconcileResponseTargetReleaseExtendAzureBinding](../models/syncreconcileresponsetargetreleaseextendazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncReconcileResponseTargetReleaseExtendAzureGrant](../models/syncreconcileresponsetargetreleaseextendazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |