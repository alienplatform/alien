# SyncReconcileResponsePreparedStackExtendGcp

GCP-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackExtendGcp } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackExtendGcp = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileResponsePreparedStackExtendGcpBinding](../models/syncreconcileresponsepreparedstackextendgcpbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncReconcileResponsePreparedStackExtendGcpGrant](../models/syncreconcileresponsepreparedstackextendgcpgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |