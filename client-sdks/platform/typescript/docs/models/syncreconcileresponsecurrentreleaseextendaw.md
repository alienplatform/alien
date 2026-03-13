# SyncReconcileResponseCurrentReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseExtendAw } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileResponseCurrentReleaseExtendAwBinding](../models/syncreconcileresponsecurrentreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncReconcileResponseCurrentReleaseExtendAwGrant](../models/syncreconcileresponsecurrentreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |