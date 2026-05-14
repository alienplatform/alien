# SyncReconcileResponseCurrentReleaseProfileAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseProfileAw } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseProfileAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                      | [models.SyncReconcileResponseCurrentReleaseProfileAwBinding](../models/syncreconcileresponsecurrentreleaseprofileawbinding.md) | :heavy_check_mark:                                                                                                             | Generic binding configuration for permissions                                                                                  |
| `effect`                                                                                                                       | [models.SyncReconcileResponseCurrentReleaseProfileEffect](../models/syncreconcileresponsecurrentreleaseprofileeffect.md)       | :heavy_minus_sign:                                                                                                             | IAM effect. Defaults to Allow.                                                                                                 |
| `grant`                                                                                                                        | [models.SyncReconcileResponseCurrentReleaseProfileAwGrant](../models/syncreconcileresponsecurrentreleaseprofileawgrant.md)     | :heavy_check_mark:                                                                                                             | Grant permissions for a specific cloud platform                                                                                |