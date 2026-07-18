# SyncAcquireResponseDeploymentTargetReleaseOverrideAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseOverrideAw } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseOverrideAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                      | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideAwBinding](../models/syncacquireresponsedeploymenttargetreleaseoverrideawbinding.md) | :heavy_check_mark:                                                                                                                             | Generic binding configuration for permissions                                                                                                  |
| `description`                                                                                                                                  | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Short admin-facing description of why this entry exists.                                                                                       |
| `effect`                                                                                                                                       | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideEffect](../models/syncacquireresponsedeploymenttargetreleaseoverrideeffect.md)       | :heavy_minus_sign:                                                                                                                             | IAM effect. Defaults to Allow.                                                                                                                 |
| `grant`                                                                                                                                        | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideAwGrant](../models/syncacquireresponsedeploymenttargetreleaseoverrideawgrant.md)     | :heavy_check_mark:                                                                                                                             | Grant permissions for a specific cloud platform                                                                                                |
| `label`                                                                                                                                        | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Stable admin-facing label for this permission entry.                                                                                           |