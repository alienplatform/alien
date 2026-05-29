# ManagerDeployment

## Example Usage

```typescript
import { ManagerDeployment } from "@alienplatform/platform-api/models";

let value: ManagerDeployment = {
  platform: "<value>",
  status: "updating",
  deploymentId: "<id>",
  currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  resources: {
    "key": {
      type: "<value>",
      status: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  | Example                                                                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `platform`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Platform of the internal deployment                                                          |                                                                                              |
| `status`                                                                                     | [models.ManagerDeploymentStatus](../models/managerdeploymentstatus.md)                       | :heavy_check_mark:                                                                           | Deployment status of the internal deployment                                                 |                                                                                              |
| `deploymentId`                                                                               | *string*                                                                                     | :heavy_check_mark:                                                                           | Internal deployment ID                                                                       |                                                                                              |
| `currentReleaseId`                                                                           | *string*                                                                                     | :heavy_minus_sign:                                                                           | Currently deployed private manager release                                                   | rel_WbhQgksrawSKIpEN0NAssHX9                                                                 |
| `desiredReleaseId`                                                                           | *string*                                                                                     | :heavy_minus_sign:                                                                           | Target private manager release for an in-progress update                                     | rel_WbhQgksrawSKIpEN0NAssHX9                                                                 |
| `error`                                                                                      | *any*                                                                                        | :heavy_minus_sign:                                                                           | Latest provision / upgrade / delete error                                                    |                                                                                              |
| `resources`                                                                                  | Record<string, [models.ManagerDeploymentResources](../models/managerdeploymentresources.md)> | :heavy_check_mark:                                                                           | Simplified stack state resources                                                             |                                                                                              |
| `environmentInfo`                                                                            | *any*                                                                                        | :heavy_minus_sign:                                                                           | Manager environment info                                                                     |                                                                                              |