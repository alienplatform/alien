# SyncReconcileResponseTargetReleaseOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseOverrideAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileResponseTargetReleaseOverrideAwBinding](../models/syncreconcileresponsetargetreleaseoverrideawbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileResponseTargetReleaseOverrideAwGrant](../models/syncreconcileresponsetargetreleaseoverrideawgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |