# ReconcileResponse

## Example Usage

```typescript
import { ReconcileResponse } from "@alienplatform/manager-api/models";

let value: ReconcileResponse = {
  current: {
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
    platform: "azure",
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
    stackState: null,
    status: "provisioning",
    targetRelease: null,
  },
  success: false,
};
```

## Fields

| Field                                                                                                                                                                                                                                                                                                                                       | Type                                                                                                                                                                                                                                                                                                                                        | Required                                                                                                                                                                                                                                                                                                                                    | Description                                                                                                                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `current`                                                                                                                                                                                                                                                                                                                                   | [models.DeploymentState](../models/deploymentstate.md)                                                                                                                                                                                                                                                                                      | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                          | Deployment state<br/><br/>Represents the current state of deployed infrastructure, including release tracking.<br/>This is platform-agnostic - no backend IDs or database relationships.<br/><br/>The deployment engine manages releases internally: when a deployment succeeds,<br/>it promotes `target_release` to `current_release` and clears `target_release`. |
| `success`                                                                                                                                                                                                                                                                                                                                   | *boolean*                                                                                                                                                                                                                                                                                                                                   | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                          | N/A                                                                                                                                                                                                                                                                                                                                         |