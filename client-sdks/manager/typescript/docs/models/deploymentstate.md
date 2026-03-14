# DeploymentState

Deployment state

Represents the current state of deployed infrastructure, including release tracking.
This is platform-agnostic - no backend IDs or database relationships.

The deployment engine manages releases internally: when a deployment succeeds,
it promotes `target_release` to `current_release` and clears `target_release`.

## Example Usage

```typescript
import { DeploymentState } from "@alienplatform/manager-api/models";

let value: DeploymentState = {
  currentRelease: {
    releaseId: "<id>",
    stack: {
      id: "<id>",
      resources: {
        "key": {
          config: {
            id: "<id>",
            type: "function",
          },
          dependencies: [],
          lifecycle: "live-on-setup",
        },
      },
    },
  },
  platform: "kubernetes",
  runtimeMetadata: {
    preparedStack: {
      id: "<id>",
      resources: {
        "key": {
          config: {
            id: "<id>",
            type: "function",
          },
          dependencies: [],
          lifecycle: "live-on-setup",
        },
      },
    },
  },
  stackState: {
    platform: "test",
    resourcePrefix: "<value>",
    resources: {},
  },
  status: "running",
  targetRelease: {
    releaseId: "<id>",
    stack: {
      id: "<id>",
      resources: {
        "key": {
          config: {
            id: "<id>",
            type: "function",
          },
          dependencies: [],
          lifecycle: "live-on-setup",
        },
      },
    },
  },
};
```

## Fields

| Field                                                                                                                                                | Type                                                                                                                                                 | Required                                                                                                                                             | Description                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `currentRelease`                                                                                                                                     | [models.ReleaseInfo](../models/releaseinfo.md)                                                                                                       | :heavy_minus_sign:                                                                                                                                   | N/A                                                                                                                                                  |
| `environmentInfo`                                                                                                                                    | *models.EnvironmentInfo*                                                                                                                             | :heavy_minus_sign:                                                                                                                                   | N/A                                                                                                                                                  |
| `platform`                                                                                                                                           | [models.PlatformEnum](../models/platformenum.md)                                                                                                     | :heavy_check_mark:                                                                                                                                   | Represents the target cloud platform.                                                                                                                |
| `retryRequested`                                                                                                                                     | *boolean*                                                                                                                                            | :heavy_minus_sign:                                                                                                                                   | Whether a retry has been requested for a failed deployment<br/>When true and status is a failed state, the deployment system will retry failed resources |
| `runtimeMetadata`                                                                                                                                    | [models.RuntimeMetadata](../models/runtimemetadata.md)                                                                                               | :heavy_minus_sign:                                                                                                                                   | N/A                                                                                                                                                  |
| `stackState`                                                                                                                                         | [models.StackState](../models/stackstate.md)                                                                                                         | :heavy_minus_sign:                                                                                                                                   | N/A                                                                                                                                                  |
| `status`                                                                                                                                             | [models.DeploymentStatus](../models/deploymentstatus.md)                                                                                             | :heavy_check_mark:                                                                                                                                   | Deployment status in the deployment lifecycle                                                                                                        |
| `targetRelease`                                                                                                                                      | [models.ReleaseInfo](../models/releaseinfo.md)                                                                                                       | :heavy_minus_sign:                                                                                                                                   | N/A                                                                                                                                                  |