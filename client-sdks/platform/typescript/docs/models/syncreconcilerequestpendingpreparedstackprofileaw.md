# SyncReconcileRequestPendingPreparedStackProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackProfileAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                | [models.SyncReconcileRequestPendingPreparedStackProfileAwBinding](../models/syncreconcilerequestpendingpreparedstackprofileawbinding.md) | :heavy_check_mark:                                                                                                                       | Generic binding configuration for permissions                                                                                            |
| `description`                                                                                                                            | *string*                                                                                                                                 | :heavy_minus_sign:                                                                                                                       | Short admin-facing description of why this entry exists.                                                                                 |
| `effect`                                                                                                                                 | [models.SyncReconcileRequestPendingPreparedStackProfileEffect](../models/syncreconcilerequestpendingpreparedstackprofileeffect.md)       | :heavy_minus_sign:                                                                                                                       | IAM effect. Defaults to Allow.                                                                                                           |
| `grant`                                                                                                                                  | [models.SyncReconcileRequestPendingPreparedStackProfileAwGrant](../models/syncreconcilerequestpendingpreparedstackprofileawgrant.md)     | :heavy_check_mark:                                                                                                                       | Grant permissions for a specific cloud platform                                                                                          |
| `label`                                                                                                                                  | *string*                                                                                                                                 | :heavy_minus_sign:                                                                                                                       | Stable admin-facing label for this permission entry.                                                                                     |
