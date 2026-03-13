# SyncReconcileResponseCurrentReleaseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseOverrideAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                              | [models.SyncReconcileResponseCurrentReleaseOverrideAzureBinding](../models/syncreconcileresponsecurrentreleaseoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                     | Generic binding configuration for permissions                                                                                          |
| `grant`                                                                                                                                | [models.SyncReconcileResponseCurrentReleaseOverrideAzureGrant](../models/syncreconcileresponsecurrentreleaseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                     | Grant permissions for a specific cloud platform                                                                                        |