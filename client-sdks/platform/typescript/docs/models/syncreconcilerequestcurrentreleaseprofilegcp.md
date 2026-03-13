# SyncReconcileRequestCurrentReleaseProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseProfileGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileRequestCurrentReleaseProfileGcpBinding](../models/syncreconcilerequestcurrentreleaseprofilegcpbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileRequestCurrentReleaseProfileGcpGrant](../models/syncreconcilerequestcurrentreleaseprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |