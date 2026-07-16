# SyncAcquireResponseDeploymentTargetReleaseOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseOverride } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseOverride = {
  description:
    "trick angrily supposing finally emulsify airbrush beside youthfully hence about",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                                  | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Human-readable description of what this permission set allows                                                                                  |
| `id`                                                                                                                                           | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Unique identifier for the permission set (e.g., "storage/data-read")                                                                           |
| `platforms`                                                                                                                                    | [models.SyncAcquireResponseDeploymentTargetReleaseOverridePlatforms](../models/syncacquireresponsedeploymenttargetreleaseoverrideplatforms.md) | :heavy_check_mark:                                                                                                                             | Platform-specific permission configurations                                                                                                    |