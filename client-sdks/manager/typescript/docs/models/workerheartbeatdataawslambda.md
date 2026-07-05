# WorkerHeartbeatDataAwsLambda

## Example Usage

```typescript
import { WorkerHeartbeatDataAwsLambda } from "@alienplatform/manager-api/models";

let value: WorkerHeartbeatDataAwsLambda = {
  functionName: "<value>",
  functionUrlCorsPresent: true,
  layerCount: 637035,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  triggerCount: 109377,
  backend: "awsLambda",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `codeSha256`                                                           | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `functionName`                                                         | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `functionUrlAuthType`                                                  | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `functionUrlCorsPresent`                                               | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `lastModified`                                                         | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `lastUpdateStatus`                                                     | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `lastUpdateStatusReason`                                               | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `lastUpdateStatusReasonCode`                                           | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `layerCount`                                                           | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `memorySizeMb`                                                         | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `packageType`                                                          | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `revisionId`                                                           | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `runtime`                                                              | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `state`                                                                | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `stateReason`                                                          | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `stateReasonCode`                                                      | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `status`                                                               | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `timeoutSeconds`                                                       | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `triggerCount`                                                         | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `version`                                                              | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `backend`                                                              | *"awsLambda"*                                                          | :heavy_check_mark:                                                     | N/A                                                                    |