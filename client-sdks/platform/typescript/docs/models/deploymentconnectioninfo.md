# DeploymentConnectionInfo

## Example Usage

```typescript
import { DeploymentConnectionInfo } from "@aliendotdev/platform-api/models";

let value: DeploymentConnectionInfo = {
  arc: {
    url: "https://nautical-doubter.net",
    deploymentId: "<id>",
  },
  resources: {},
  status: "refresh-failed",
  platform: "<value>",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `arc`                                                                                                      | [models.Arc](../models/arc.md)                                                                             | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `resources`                                                                                                | Record<string, [models.DeploymentConnectionInfoResources](../models/deploymentconnectioninforesources.md)> | :heavy_check_mark:                                                                                         | Deployed resources and their URLs                                                                          |
| `status`                                                                                                   | [models.DeploymentConnectionInfoStatus](../models/deploymentconnectioninfostatus.md)                       | :heavy_check_mark:                                                                                         | Deployment status in the deployment lifecycle                                                              |
| `platform`                                                                                                 | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |