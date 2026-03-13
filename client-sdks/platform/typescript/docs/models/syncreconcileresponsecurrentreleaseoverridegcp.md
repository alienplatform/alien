# SyncReconcileResponseCurrentReleaseOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseOverrideGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncReconcileResponseCurrentReleaseOverrideGcpBinding](../models/syncreconcileresponsecurrentreleaseoverridegcpbinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncReconcileResponseCurrentReleaseOverrideGcpGrant](../models/syncreconcileresponsecurrentreleaseoverridegcpgrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |