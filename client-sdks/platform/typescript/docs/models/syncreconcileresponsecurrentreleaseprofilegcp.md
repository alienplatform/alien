# SyncReconcileResponseCurrentReleaseProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseProfileGcp } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncReconcileResponseCurrentReleaseProfileGcpBinding](../models/syncreconcileresponsecurrentreleaseprofilegcpbinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncReconcileResponseCurrentReleaseProfileGcpGrant](../models/syncreconcileresponsecurrentreleaseprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |