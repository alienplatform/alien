# SyncReconcileRequestPreparedStackProfileGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackProfileGcp } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackProfileGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileRequestPreparedStackProfileGcpBinding](../models/syncreconcilerequestpreparedstackprofilegcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncReconcileRequestPreparedStackProfileGcpGrant](../models/syncreconcilerequestpreparedstackprofilegcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |