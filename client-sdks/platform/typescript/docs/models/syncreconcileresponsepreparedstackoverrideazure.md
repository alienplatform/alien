# SyncReconcileResponsePreparedStackOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackOverrideAzure } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                            | [models.SyncReconcileResponsePreparedStackOverrideAzureBinding](../models/syncreconcileresponsepreparedstackoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                   | Generic binding configuration for permissions                                                                                        |
| `grant`                                                                                                                              | [models.SyncReconcileResponsePreparedStackOverrideAzureGrant](../models/syncreconcileresponsepreparedstackoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                   | Grant permissions for a specific cloud platform                                                                                      |