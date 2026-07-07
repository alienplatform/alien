# DataAwsSqs

## Example Usage

```typescript
import { DataAwsSqs } from "@alienplatform/platform-api/models/operations";

let value: DataAwsSqs = {
  approximateCounts: false,
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
  backend: "awsSqs",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `approximateCounts`                                                | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `approximateDelayedMessages`                                       | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `approximateInFlightMessages`                                      | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `approximateVisibleMessages`                                       | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `contentBasedDeduplication`                                        | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `deduplicationScope`                                               | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `delaySeconds`                                                     | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `fifoQueue`                                                        | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `fifoThroughputLimit`                                              | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `kmsDataKeyReusePeriodSeconds`                                     | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `kmsMasterKeyId`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `maximumMessageSize`                                               | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `messageRetentionPeriodSeconds`                                    | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `name`                                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `queueArn`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `queueUrl`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `receiveMessageWaitTimeSeconds`                                    | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `redriveAllowPolicy`                                               | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `redrivePolicy`                                                    | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `region`                                                           | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `sqsManagedSseEnabled`                                             | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `sseEnabled`                                                       | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus25](../../models/operations/datastatus25.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `visibilityTimeoutSeconds`                                         | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"awsSqs"*                                                         | :heavy_check_mark:                                                 | N/A                                                                |