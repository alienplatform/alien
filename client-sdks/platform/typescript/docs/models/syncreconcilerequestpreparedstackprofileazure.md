# SyncReconcileRequestPreparedStackProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackProfileAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestPreparedStackProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                        | [models.SyncReconcileRequestPreparedStackProfileAzureBinding](../models/syncreconcilerequestpreparedstackprofileazurebinding.md) | :heavy_check_mark:                                                                                                               | Generic binding configuration for permissions                                                                                    |
| `grant`                                                                                                                          | [models.SyncReconcileRequestPreparedStackProfileAzureGrant](../models/syncreconcilerequestpreparedstackprofileazuregrant.md)     | :heavy_check_mark:                                                                                                               | Grant permissions for a specific cloud platform                                                                                  |