# SyncReconcileRequestCurrentReleaseExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseExtendGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileRequestCurrentReleaseExtendGcpBinding](../models/syncreconcilerequestcurrentreleaseextendgcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncReconcileRequestCurrentReleaseExtendGcpGrant](../models/syncreconcilerequestcurrentreleaseextendgcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |