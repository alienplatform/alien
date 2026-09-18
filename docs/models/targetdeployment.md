# TargetDeployment

Target deployment if update is needed

## Example Usage

```typescript
import { TargetDeployment } from "@alienplatform/platform-api/models";

let value: TargetDeployment = {
  config: {
    environmentVariables: {
      createdAt: "1713608778171",
      hash: "<value>",
      variables: [],
    },
  },
  releaseInfo: {
    stack: {
      id: "<id>",
      resources: {
        "key": {
          config: {
            id: "<id>",
            type: "<value>",
          },
          dependencies: [
            {
              id: "<id>",
              type: "<value>",
            },
          ],
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
| `config`                                                                                                                                                                                         | [models.TargetDeploymentConfig](../models/targetdeploymentconfig.md)                                                                                                                             | :heavy_check_mark:                                                                                                                                                                               | Deployment configuration<br/><br/>Configuration for how to perform the deployment.<br/>Note: Credentials (ClientConfig) are passed separately to step() function.                                |
| `releaseInfo`                                                                                                                                                                                    | [models.ReleaseInfo](../models/releaseinfo.md)                                                                                                                                                   | :heavy_check_mark:                                                                                                                                                                               | Release metadata<br/><br/>Identifies a specific release version and includes the stack definition.<br/>The deployment engine uses this to track which release is currently deployed<br/>and which is the target. |