# SyncReconcileResponseTargetReleaseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseOverrideAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTargetReleaseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                            | [models.SyncReconcileResponseTargetReleaseOverrideAzureBinding](../models/syncreconcileresponsetargetreleaseoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                   | Generic binding configuration for permissions                                                                                        |
| `grant`                                                                                                                              | [models.SyncReconcileResponseTargetReleaseOverrideAzureGrant](../models/syncreconcileresponsetargetreleaseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                   | Grant permissions for a specific cloud platform                                                                                      |