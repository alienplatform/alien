# SyncAcquireResponseDeploymentPendingPreparedStackOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPendingPreparedStackOverride } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPendingPreparedStackOverride = {
  description: "past huzzah which er plus or unripe vaguely phew deploy",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                                                        | Type                                                                                                                                                         | Required                                                                                                                                                     | Description                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `description`                                                                                                                                                | *string*                                                                                                                                                     | :heavy_check_mark:                                                                                                                                           | Human-readable description of what this permission set allows                                                                                                |
| `id`                                                                                                                                                         | *string*                                                                                                                                                     | :heavy_check_mark:                                                                                                                                           | Unique identifier for the permission set (e.g., "storage/data-read")                                                                                         |
| `platforms`                                                                                                                                                  | [models.SyncAcquireResponseDeploymentPendingPreparedStackOverridePlatforms](../models/syncacquireresponsedeploymentpendingpreparedstackoverrideplatforms.md) | :heavy_check_mark:                                                                                                                                           | Platform-specific permission configurations                                                                                                                  |
