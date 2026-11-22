# ManagerRetryDeploymentResponse

## Example Usage

```typescript
import { ManagerRetryDeploymentResponse } from "@alienplatform/platform-api/models";

let value: ManagerRetryDeploymentResponse = {
  mode: "deployment",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  setupStatus: "provisioning",
  deploymentId: "<id>",
  message: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  | Example                                                                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `mode`                                                                                       | [models.ManagerRetryDeploymentResponseMode](../models/managerretrydeploymentresponsemode.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |                                                                                              |
| `managerId`                                                                                  | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          | mgr_enxscjrqiiu2lrc672hwwuc5                                                                 |
| `setupStatus`                                                                                | *"provisioning"*                                                                             | :heavy_check_mark:                                                                           | N/A                                                                                          |                                                                                              |
| `deploymentId`                                                                               | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |                                                                                              |
| `message`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |                                                                                              |