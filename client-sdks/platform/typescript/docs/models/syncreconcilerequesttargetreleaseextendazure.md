# SyncReconcileRequestTargetReleaseExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseExtendAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileRequestTargetReleaseExtendAzureBinding](../models/syncreconcilerequesttargetreleaseextendazurebinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileRequestTargetReleaseExtendAzureGrant](../models/syncreconcilerequesttargetreleaseextendazuregrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |