# SyncReconcileResponsePreparedStackOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackOverrideAw } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePreparedStackOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileResponsePreparedStackOverrideAwBinding](../models/syncreconcileresponsepreparedstackoverrideawbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileResponsePreparedStackOverrideAwGrant](../models/syncreconcileresponsepreparedstackoverrideawgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |