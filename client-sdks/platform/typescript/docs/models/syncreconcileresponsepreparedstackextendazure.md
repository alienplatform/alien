# SyncReconcileResponsePreparedStackExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncReconcileResponsePreparedStackExtendAzureBinding](../models/syncreconcileresponsepreparedstackextendazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncReconcileResponsePreparedStackExtendAzureGrant](../models/syncreconcileresponsepreparedstackextendazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |