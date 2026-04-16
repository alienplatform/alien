# SyncReconcileResponseTarget

Target deployment if update is needed

## Example Usage

```typescript
import { SyncReconcileResponseTarget } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTarget = {
  config: {
    environmentVariables: {
      createdAt: "1707747302593",
      hash: "<value>",
      variables: [],
    },
  },
  releaseInfo: {
    releaseId: "<id>",
    stack: {
      id: "<id>",
      resources: {
        "key": {
          config: {
            id: "<id>",
            type: "<value>",
          },
          dependencies: [],
          lifecycle: "frozen",
        },
      },
    },
  },
};
```

## Fields

| Field                                                                                                                                                                                            | Type                                                                                                                                                                                             | Required                                                                                                                                                                                         | Description                                                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `config`                                                                                                                                                                                         | [models.TargetConfig](../models/targetconfig.md)                                                                                                                                                 | :heavy_check_mark:                                                                                                                                                                               | Deployment configuration<br/><br/>Configuration for how to perform the deployment.<br/>Note: Credentials (ClientConfig) are passed separately to step() function.                                |
| `releaseInfo`                                                                                                                                                                                    | [models.ReleaseInfo](../models/releaseinfo.md)                                                                                                                                                   | :heavy_check_mark:                                                                                                                                                                               | Release metadata<br/><br/>Identifies a specific release version and includes the stack definition.<br/>The deployment engine uses this to track which release is currently deployed<br/>and which is the target. |