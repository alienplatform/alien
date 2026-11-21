# DeploymentPendingPreparedStackPermissionSet

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { DeploymentPendingPreparedStackPermissionSet } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackPermissionSet = {
  description:
    "teriyaki besides sympathetically versus lest boastfully consequently dime hourly twist",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                                    | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | Human-readable description of what this permission set allows                                                                    |
| `id`                                                                                                                             | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | Unique identifier for the permission set (e.g., "storage/data-read")                                                             |
| `platforms`                                                                                                                      | [models.DeploymentPendingPreparedStackPermissionSetPlatforms](../models/deploymentpendingpreparedstackpermissionsetplatforms.md) | :heavy_check_mark:                                                                                                               | Platform-specific permission configurations                                                                                      |