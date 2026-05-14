# SyncReconcileRequestPreparedStackProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackProfileAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                  | [models.SyncReconcileRequestPreparedStackProfileAwBinding](../models/syncreconcilerequestpreparedstackprofileawbinding.md) | :heavy_check_mark:                                                                                                         | Generic binding configuration for permissions                                                                              |
| `effect`                                                                                                                   | [models.SyncReconcileRequestPreparedStackProfileEffect](../models/syncreconcilerequestpreparedstackprofileeffect.md)       | :heavy_minus_sign:                                                                                                         | IAM effect. Defaults to Allow.                                                                                             |
| `grant`                                                                                                                    | [models.SyncReconcileRequestPreparedStackProfileAwGrant](../models/syncreconcilerequestpreparedstackprofileawgrant.md)     | :heavy_check_mark:                                                                                                         | Grant permissions for a specific cloud platform                                                                            |