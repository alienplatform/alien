# SyncReconcileResponseTargetReleaseProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseProfileAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                    | [models.SyncReconcileResponseTargetReleaseProfileAwBinding](../models/syncreconcileresponsetargetreleaseprofileawbinding.md) | :heavy_check_mark:                                                                                                           | Generic binding configuration for permissions                                                                                |
| `grant`                                                                                                                      | [models.SyncReconcileResponseTargetReleaseProfileAwGrant](../models/syncreconcileresponsetargetreleaseprofileawgrant.md)     | :heavy_check_mark:                                                                                                           | Grant permissions for a specific cloud platform                                                                              |