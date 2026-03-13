# SyncReconcileRequestTargetReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseExtendAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                | [models.SyncReconcileRequestTargetReleaseExtendAwBinding](../models/syncreconcilerequesttargetreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                       | Generic binding configuration for permissions                                                                            |
| `grant`                                                                                                                  | [models.SyncReconcileRequestTargetReleaseExtendAwGrant](../models/syncreconcilerequesttargetreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                       | Grant permissions for a specific cloud platform                                                                          |