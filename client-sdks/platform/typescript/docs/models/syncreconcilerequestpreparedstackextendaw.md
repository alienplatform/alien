# SyncReconcileRequestPreparedStackExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackExtendAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncReconcileRequestPreparedStackExtendAwBinding](../models/syncreconcilerequestpreparedstackextendawbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.SyncReconcileRequestPreparedStackExtendAwGrant](../models/syncreconcilerequestpreparedstackextendawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |