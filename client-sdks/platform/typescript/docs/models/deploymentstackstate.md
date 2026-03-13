# DeploymentStackState

State of infrastructure components managed by this deployment

## Example Usage

```typescript
import { DeploymentStackState } from "@aliendotdev/platform-api/models";

let value: DeploymentStackState = {
  platform: "local",
  resourcePrefix: "<value>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      status: "provision-failed",
      type: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `platform`                                                                                         | [models.DeploymentStackStatePlatform](../models/deploymentstackstateplatform.md)                   | :heavy_check_mark:                                                                                 | Represents the target cloud platform.                                                              |
| `resourcePrefix`                                                                                   | *string*                                                                                           | :heavy_check_mark:                                                                                 | A prefix used for resource naming to ensure uniqueness across deployments.                         |
| `resources`                                                                                        | Record<string, [models.DeploymentStackStateResources](../models/deploymentstackstateresources.md)> | :heavy_check_mark:                                                                                 | The state of individual resources, keyed by resource ID.                                           |