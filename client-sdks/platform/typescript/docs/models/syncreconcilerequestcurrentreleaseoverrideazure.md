# SyncReconcileRequestCurrentReleaseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseOverrideAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                            | [models.SyncReconcileRequestCurrentReleaseOverrideAzureBinding](../models/syncreconcilerequestcurrentreleaseoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                   | Generic binding configuration for permissions                                                                                        |
| `grant`                                                                                                                              | [models.SyncReconcileRequestCurrentReleaseOverrideAzureGrant](../models/syncreconcilerequestcurrentreleaseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                   | Grant permissions for a specific cloud platform                                                                                      |