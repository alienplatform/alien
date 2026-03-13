# SyncReconcileResponsePreparedStackExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackExtendAw } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePreparedStackExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncReconcileResponsePreparedStackExtendAwBinding](../models/syncreconcileresponsepreparedstackextendawbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `grant`                                                                                                                    | [models.SyncReconcileResponsePreparedStackExtendAwGrant](../models/syncreconcileresponsepreparedstackextendawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |