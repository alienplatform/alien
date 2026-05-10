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
| `effect`                                                                                                                 | [models.SyncReconcileRequestPreparedStackExtendEffect](../models/syncreconcilerequestpreparedstackextendeffect.md)       | :heavy_minus_sign:                                                                                                       | IAM effect. Defaults to Allow.                                                                                           |
| `grant`                                                                                                                  | [models.SyncReconcileRequestPreparedStackExtendAwGrant](../models/syncreconcilerequestpreparedstackextendawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |