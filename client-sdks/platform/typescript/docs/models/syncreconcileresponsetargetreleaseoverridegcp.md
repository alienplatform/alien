# SyncReconcileResponseTargetReleaseOverrideGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseOverrideGcp } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseOverrideGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncReconcileResponseTargetReleaseOverrideGcpBinding](../models/syncreconcileresponsetargetreleaseoverridegcpbinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncReconcileResponseTargetReleaseOverrideGcpGrant](../models/syncreconcileresponsetargetreleaseoverridegcpgrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |