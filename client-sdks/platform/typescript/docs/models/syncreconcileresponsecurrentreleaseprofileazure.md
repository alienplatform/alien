# SyncReconcileResponseCurrentReleaseProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseProfileAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                            | [models.SyncReconcileResponseCurrentReleaseProfileAzureBinding](../models/syncreconcileresponsecurrentreleaseprofileazurebinding.md) | :heavy_check_mark:                                                                                                                   | Generic binding configuration for permissions                                                                                        |
| `grant`                                                                                                                              | [models.SyncReconcileResponseCurrentReleaseProfileAzureGrant](../models/syncreconcileresponsecurrentreleaseprofileazuregrant.md)     | :heavy_check_mark:                                                                                                                   | Grant permissions for a specific cloud platform                                                                                      |