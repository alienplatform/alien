# ReplicasInfo

## Example Usage

```typescript
import { ReplicasInfo } from "@alienplatform/platform-api/models/operations";

let value: ReplicasInfo = {
  replicaId: "<id>",
  machineId: "<id>",
  ip: "5e8b:b2ff:ab4c:3c87:b8d0:e0c4:4e13:4a4f",
  status: "<value>",
  healthy: true,
  consecutiveFailures: 70372,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `replicaId`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `machineId`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `ip`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `status`                                                                                             | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `healthy`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `consecutiveFailures`                                                                                | *number*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `metrics`                                                                                            | [operations.GetDeploymentContainerMetrics](../../models/operations/getdeploymentcontainermetrics.md) | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `additionalProperties`                                                                               | Record<string, *any*>                                                                                | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |