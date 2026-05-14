# SyncReconcileRequestTargetReleaseOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseOverrideAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileRequestTargetReleaseOverrideAwBinding](../models/syncreconcilerequesttargetreleaseoverrideawbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `effect`                                                                                                                     | [models.SyncReconcileRequestTargetReleaseOverrideEffect](../models/syncreconcilerequesttargetreleaseoverrideeffect.md)       | :heavy_minus_sign:                                                                                                           | IAM effect. Defaults to Allow.                                                                                               |
| `grant`                                                                                                                      | [models.SyncReconcileRequestTargetReleaseOverrideAwGrant](../models/syncreconcilerequesttargetreleaseoverrideawgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |