# SyncReconcileResponseTargetReleaseProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseProfileGcp } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileResponseTargetReleaseProfileGcpBinding](../models/syncreconcileresponsetargetreleaseprofilegcpbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileResponseTargetReleaseProfileGcpGrant](../models/syncreconcileresponsetargetreleaseprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |