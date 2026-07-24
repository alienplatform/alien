# DeploymentPendingPreparedStackOverride

A permission set that can be applied across different cloud platforms

## Example Usage

```typescript
import { DeploymentPendingPreparedStackOverride } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackOverride = {
  description:
    "almighty convection pip throughout hm consign impact alb blah slipper",
  id: "<id>",
  platforms: {},
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `description`                                                                                                          | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Human-readable description of what this permission set allows                                                          |
| `id`                                                                                                                   | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Unique identifier for the permission set (e.g., "storage/data-read")                                                   |
| `platforms`                                                                                                            | [models.DeploymentPendingPreparedStackOverridePlatforms](../models/deploymentpendingpreparedstackoverrideplatforms.md) | :heavy_check_mark:                                                                                                     | Platform-specific permission configurations                                                                            |
