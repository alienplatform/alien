# SyncReconcileRequestPreparedStackOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackOverrideAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileRequestPreparedStackOverrideAwBinding](../models/syncreconcilerequestpreparedstackoverrideawbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `effect`                                                                                                                     | [models.SyncReconcileRequestPreparedStackOverrideEffect](../models/syncreconcilerequestpreparedstackoverrideeffect.md)       | :heavy_minus_sign:                                                                                                           | IAM effect. Defaults to Allow.                                                                                               |
| `grant`                                                                                                                      | [models.SyncReconcileRequestPreparedStackOverrideAwGrant](../models/syncreconcilerequestpreparedstackoverrideawgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |