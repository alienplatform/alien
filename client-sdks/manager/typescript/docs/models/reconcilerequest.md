# ReconcileRequest

## Example Usage

```typescript
import { ReconcileRequest } from "@alienplatform/manager-api/models";

let value: ReconcileRequest = {
  deploymentId: "<id>",
  session: "<value>",
  state: {
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
    platform: "local",
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
    status: "update-failed",
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
  },
};
```

## Fields

| Field                                                                                                                                                                                                                                                                                                                                       | Type                                                                                                                                                                                                                                                                                                                                        | Required                                                                                                                                                                                                                                                                                                                                    | Description                                                                                                                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                                                                                                                                                                                                                                              | *string*                                                                                                                                                                                                                                                                                                                                    | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                          | N/A                                                                                                                                                                                                                                                                                                                                         |
| `session`                                                                                                                                                                                                                                                                                                                                   | *string*                                                                                                                                                                                                                                                                                                                                    | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                          | N/A                                                                                                                                                                                                                                                                                                                                         |
| `state`                                                                                                                                                                                                                                                                                                                                     | [models.DeploymentState](../models/deploymentstate.md)                                                                                                                                                                                                                                                                                      | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                          | Deployment state<br/><br/>Represents the current state of deployed infrastructure, including release tracking.<br/>This is platform-agnostic - no backend IDs or database relationships.<br/><br/>The deployment engine manages releases internally: when a deployment succeeds,<br/>it promotes `target_release` to `current_release` and clears `target_release`. |
| `updateHeartbeat`                                                                                                                                                                                                                                                                                                                           | *boolean*                                                                                                                                                                                                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                                                                                                                                                                                                          | N/A                                                                                                                                                                                                                                                                                                                                         |