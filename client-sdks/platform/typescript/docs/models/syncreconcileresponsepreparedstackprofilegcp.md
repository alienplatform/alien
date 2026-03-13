# SyncReconcileResponsePreparedStackProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackProfileGcp } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePreparedStackProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileResponsePreparedStackProfileGcpBinding](../models/syncreconcileresponsepreparedstackprofilegcpbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileResponsePreparedStackProfileGcpGrant](../models/syncreconcileresponsepreparedstackprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |