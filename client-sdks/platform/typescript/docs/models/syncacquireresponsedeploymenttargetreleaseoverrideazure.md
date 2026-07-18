# SyncAcquireResponseDeploymentTargetReleaseOverrideAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseOverrideAzure } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseOverrideAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                                                                | Type                                                                                                                                                 | Required                                                                                                                                             | Description                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                                            | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideAzureBinding](../models/syncacquireresponsedeploymenttargetreleaseoverrideazurebinding.md) | :heavy_check_mark:                                                                                                                                   | Generic binding configuration for permissions                                                                                                        |
| `description`                                                                                                                                        | *string*                                                                                                                                             | :heavy_minus_sign:                                                                                                                                   | Short admin-facing description of why this entry exists.                                                                                             |
| `grant`                                                                                                                                              | [models.SyncAcquireResponseDeploymentTargetReleaseOverrideAzureGrant](../models/syncacquireresponsedeploymenttargetreleaseoverrideazuregrant.md)     | :heavy_check_mark:                                                                                                                                   | Grant permissions for a specific cloud platform                                                                                                      |
| `label`                                                                                                                                              | *string*                                                                                                                                             | :heavy_minus_sign:                                                                                                                                   | Stable admin-facing label for this permission entry.                                                                                                 |