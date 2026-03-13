# SyncReconcileResponsePreparedStackProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackProfileAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncReconcileResponsePreparedStackProfileAzureBinding](../models/syncreconcileresponsepreparedstackprofileazurebinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncReconcileResponsePreparedStackProfileAzureGrant](../models/syncreconcileresponsepreparedstackprofileazuregrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |