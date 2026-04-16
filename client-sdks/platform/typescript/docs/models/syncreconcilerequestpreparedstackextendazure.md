# SyncReconcileRequestPreparedStackExtendAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackExtendAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackExtendAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileRequestPreparedStackExtendAzureBinding](../models/syncreconcilerequestpreparedstackextendazurebinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileRequestPreparedStackExtendAzureGrant](../models/syncreconcilerequestpreparedstackextendazuregrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |