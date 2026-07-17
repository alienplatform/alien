# SyncAcquireResponseDeploymentStackState

Represents the collective state of all resources in a stack, including platform and pending actions.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentStackState } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentStackState = {
  platform: "azure",
  resourcePrefix: "<value>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      status: "running",
      type: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                                               | [models.SyncAcquireResponseDeploymentStackStatePlatform](../models/syncacquireresponsedeploymentstackstateplatform.md)                   | :heavy_check_mark:                                                                                                                       | Represents the target cloud platform.                                                                                                    |
| `resourcePrefix`                                                                                                                         | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | A prefix used for resource naming to ensure uniqueness across deployments.                                                               |
| `resources`                                                                                                                              | Record<string, [models.SyncAcquireResponseDeploymentStackStateResources](../models/syncacquireresponsedeploymentstackstateresources.md)> | :heavy_check_mark:                                                                                                                       | The state of individual resources, keyed by resource ID.                                                                                 |