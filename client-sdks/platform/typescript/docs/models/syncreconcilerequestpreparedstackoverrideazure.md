# SyncReconcileRequestPreparedStackOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackOverrideAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                          | [models.SyncReconcileRequestPreparedStackOverrideAzureBinding](../models/syncreconcilerequestpreparedstackoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                 | Generic binding configuration for permissions                                                                                      |
| `grant`                                                                                                                            | [models.SyncReconcileRequestPreparedStackOverrideAzureGrant](../models/syncreconcilerequestpreparedstackoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                 | Grant permissions for a specific cloud platform                                                                                    |