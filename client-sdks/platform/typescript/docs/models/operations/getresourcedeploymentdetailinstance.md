# GetResourceDeploymentDetailInstance

## Example Usage

```typescript
import { GetResourceDeploymentDetailInstance } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailInstance = {
  instanceId: "<id>",
  name: "<value>",
  ready: true,
  phase: "<value>",
  nodeName: "<value>",
  restartCount: 135511,
  waitingReason: "<value>",
  terminatedReason: "<value>",
  observedAt: new Date("2026-09-02T11:45:11.905Z"),
  platformStale: true,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `instanceId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `ready`                                                                                       | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `phase`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `nodeName`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `restartCount`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `waitingReason`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `terminatedReason`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `cpu`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `memory`                                                                                      | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platformStale`                                                                               | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `provider`                                                                                    | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |