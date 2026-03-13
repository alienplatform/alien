# SyncReconcileRequestCurrentReleaseOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseOverrideAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileRequestCurrentReleaseOverrideAwBinding](../models/syncreconcilerequestcurrentreleaseoverrideawbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `grant`                                                                                                                        | [models.SyncReconcileRequestCurrentReleaseOverrideAwGrant](../models/syncreconcilerequestcurrentreleaseoverrideawgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |