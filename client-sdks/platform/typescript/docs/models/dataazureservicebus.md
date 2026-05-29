# DataAzureServiceBus

## Example Usage

```typescript
import { DataAzureServiceBus } from "@alienplatform/platform-api/models";

let value: DataAzureServiceBus = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-12-11T21:43:07.631Z"),
      severity: "error",
    },
  ],
  name: "<value>",
  namespaceName: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  backend: "azureServiceBus",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `accessedAt`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `activeMessageCount`                                                             | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `autoDeleteOnIdle`                                                               | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `createdAt`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `deadLetterMessageCount`                                                         | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `deadLetteringOnMessageExpiration`                                               | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `defaultMessageTimeToLive`                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `duplicateDetectionHistoryTimeWindow`                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `enableBatchedOperations`                                                        | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `enableExpress`                                                                  | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `enablePartitioning`                                                             | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `endpoint`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent25](../models/syncreconcilerequestevent25.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `forwardDeadLetteredMessagesTo`                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `forwardTo`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `lockDuration`                                                                   | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `maxDeliveryCount`                                                               | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `maxMessageSizeInKilobytes`                                                      | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `maxSizeInMegabytes`                                                             | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `messageCount`                                                                   | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespaceName`                                                                  | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `queueStatus`                                                                    | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `requiresDuplicateDetection`                                                     | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `requiresSession`                                                                | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceGroup`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceId`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `scheduledMessageCount`                                                          | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `sizeInBytes`                                                                    | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus25](../models/heartbeatstatus25.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `transferDeadLetterMessageCount`                                                 | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `transferMessageCount`                                                           | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `updatedAt`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `backend`                                                                        | *"azureServiceBus"*                                                              | :heavy_check_mark:                                                               | N/A                                                                              |