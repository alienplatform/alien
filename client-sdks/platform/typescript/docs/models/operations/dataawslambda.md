# DataAwsLambda

## Example Usage

```typescript
import { DataAwsLambda } from "@alienplatform/platform-api/models/operations";

let value: DataAwsLambda = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-07-19T03:10:06.736Z"),
      severity: "warning",
    },
  ],
  functionName: "<value>",
  functionUrlCorsPresent: false,
  layerCount: 965000,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "scaling",
    partial: false,
    stale: false,
  },
  triggerCount: 857435,
  backend: "awsLambda",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `codeSha256`                                                                                           | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `events`                                                                                               | [operations.GetRawResourceHeartbeatEvent5](../../models/operations/getrawresourceheartbeatevent5.md)[] | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `functionName`                                                                                         | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `functionUrlAuthType`                                                                                  | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `functionUrlCorsPresent`                                                                               | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `lastModified`                                                                                         | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `lastUpdateStatus`                                                                                     | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `lastUpdateStatusReason`                                                                               | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `lastUpdateStatusReasonCode`                                                                           | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `layerCount`                                                                                           | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `memorySizeMb`                                                                                         | *number*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `packageType`                                                                                          | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `revisionId`                                                                                           | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `runtime`                                                                                              | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `state`                                                                                                | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `stateReason`                                                                                          | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `stateReasonCode`                                                                                      | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [operations.DataStatus5](../../models/operations/datastatus5.md)                                       | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `timeoutSeconds`                                                                                       | *number*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `triggerCount`                                                                                         | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `version`                                                                                              | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `backend`                                                                                              | *"awsLambda"*                                                                                          | :heavy_check_mark:                                                                                     | N/A                                                                                                    |