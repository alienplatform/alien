# SyncAcquireResponseDeploymentTargetReleaseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseExtendAw } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                                  | [models.SyncAcquireResponseDeploymentTargetReleaseExtendAwBinding](../models/syncacquireresponsedeploymenttargetreleaseextendawbinding.md) | :heavy_check_mark:                                                                                                                         | Generic binding configuration for permissions                                                                                              |
| `description`                                                                                                                              | *string*                                                                                                                                   | :heavy_minus_sign:                                                                                                                         | Short admin-facing description of why this entry exists.                                                                                   |
| `effect`                                                                                                                                   | [models.SyncAcquireResponseDeploymentTargetReleaseExtendEffect](../models/syncacquireresponsedeploymenttargetreleaseextendeffect.md)       | :heavy_minus_sign:                                                                                                                         | IAM effect. Defaults to Allow.                                                                                                             |
| `grant`                                                                                                                                    | [models.SyncAcquireResponseDeploymentTargetReleaseExtendAwGrant](../models/syncacquireresponsedeploymenttargetreleaseextendawgrant.md)     | :heavy_check_mark:                                                                                                                         | Grant permissions for a specific cloud platform                                                                                            |
| `label`                                                                                                                                    | *string*                                                                                                                                   | :heavy_minus_sign:                                                                                                                         | Stable admin-facing label for this permission entry.                                                                                       |