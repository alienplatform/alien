# Instance3

## Example Usage

```typescript
import { Instance3 } from "@alienplatform/platform-api/models";

let value: Instance3 = {
  name: "<value>",
  ready: false,
  replicaId: "<id>",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `cpu`                         | *models.InstanceCpuUnion3*    | :heavy_minus_sign:            | N/A                           |
| `ip`                          | *string*                      | :heavy_minus_sign:            | N/A                           |
| `machineId`                   | *string*                      | :heavy_minus_sign:            | N/A                           |
| `memory`                      | *models.InstanceMemoryUnion3* | :heavy_minus_sign:            | N/A                           |
| `message`                     | *string*                      | :heavy_minus_sign:            | N/A                           |
| `metricsHealthy`              | *boolean*                     | :heavy_minus_sign:            | N/A                           |
| `metricsLastUpdated`          | *string*                      | :heavy_minus_sign:            | N/A                           |
| `metricsStatus`               | *string*                      | :heavy_minus_sign:            | N/A                           |
| `name`                        | *string*                      | :heavy_check_mark:            | N/A                           |
| `nodeName`                    | *string*                      | :heavy_minus_sign:            | N/A                           |
| `phase`                       | *string*                      | :heavy_minus_sign:            | N/A                           |
| `ready`                       | *boolean*                     | :heavy_check_mark:            | N/A                           |
| `reason`                      | *string*                      | :heavy_minus_sign:            | N/A                           |
| `replicaId`                   | *string*                      | :heavy_check_mark:            | N/A                           |
| `restartCount`                | *number*                      | :heavy_minus_sign:            | N/A                           |
| `status`                      | *string*                      | :heavy_minus_sign:            | N/A                           |
| `terminatedReason`            | *string*                      | :heavy_minus_sign:            | N/A                           |
| `waitingReason`               | *string*                      | :heavy_minus_sign:            | N/A                           |