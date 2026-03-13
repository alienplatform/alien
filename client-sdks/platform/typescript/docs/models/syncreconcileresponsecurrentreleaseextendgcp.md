# SyncReconcileResponseCurrentReleaseExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseExtendGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileResponseCurrentReleaseExtendGcpBinding](../models/syncreconcileresponsecurrentreleaseextendgcpbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileResponseCurrentReleaseExtendGcpGrant](../models/syncreconcileresponsecurrentreleaseextendgcpgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |