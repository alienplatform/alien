# ManagerDeployment

## Example Usage

```typescript
import { ManagerDeployment } from "@aliendotdev/platform-api/models";

let value: ManagerDeployment = {
  platform: "<value>",
  status: "updating",
  resources: {
    "key": {
      type: "<value>",
      status: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `platform`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Platform of the internal agent                                                               |
| `status`                                                                                     | [models.ManagerDeploymentStatus](../models/managerdeploymentstatus.md)                       | :heavy_check_mark:                                                                           | Deployment status of the internal agent                                                      |
| `error`                                                                                      | *any*                                                                                        | :heavy_minus_sign:                                                                           | Latest provision / upgrade / delete error                                                    |
| `resources`                                                                                  | Record<string, [models.ManagerDeploymentResources](../models/managerdeploymentresources.md)> | :heavy_check_mark:                                                                           | Simplified stack state resources                                                             |
| `environmentInfo`                                                                            | *any*                                                                                        | :heavy_minus_sign:                                                                           | Manager environment info                                                                     |