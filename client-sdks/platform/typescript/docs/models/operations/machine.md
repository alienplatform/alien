# Machine

## Example Usage

```typescript
import { Machine } from "@alienplatform/platform-api/models/operations";

let value: Machine = {
  deploymentId: "<id>",
  deploymentName: "<value>",
  deploymentGroupId: "<id>",
  deploymentGroupName: "<value>",
  machineId: "<id>",
  name: "<value>",
  backend: "<value>",
  controllerPlatform: "<value>",
  ready: false,
  roles: [],
  labels: {
    "key": "<value>",
  },
  kubeletVersion: "<value>",
  containerRuntimeVersion: "<value>",
  observedAt: new Date("2025-02-16T02:09:58.768Z"),
  platformStale: false,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentName`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentGroupName`                                                                         | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `machineId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `controllerPlatform`                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `ready`                                                                                       | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `roles`                                                                                       | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `labels`                                                                                      | Record<string, *string*>                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `capacity`                                                                                    | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `allocatable`                                                                                 | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `usage`                                                                                       | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `kubeletVersion`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `containerRuntimeVersion`                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platformStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `provider`                                                                                    | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |