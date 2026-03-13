# SyncReconcileRequestTargetReleaseProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseProfileGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileRequestTargetReleaseProfileGcpBinding](../models/syncreconcilerequesttargetreleaseprofilegcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncReconcileRequestTargetReleaseProfileGcpGrant](../models/syncreconcilerequesttargetreleaseprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |