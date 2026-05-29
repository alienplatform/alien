# QueueHeartbeatDataGcpPubSub

## Example Usage

```typescript
import { QueueHeartbeatDataGcpPubSub } from "@alienplatform/manager-api/models";

let value: QueueHeartbeatDataGcpPubSub = {
  events: [],
  messageStorageAllowedPersistenceRegions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  subscriptionLabels: {
    "key": "<value>",
  },
  subscriptionPushAttributes: {
    "key": "<value>",
    "key1": "<value>",
  },
  topicLabels: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  topicName: "<value>",
  backend: "gcpPubSub",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `endpoint`                                                       | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `events`                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]           | :heavy_check_mark:                                               | N/A                                                              |
| `kmsKeyName`                                                     | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `messageStorageAllowedPersistenceRegions`                        | *string*[]                                                       | :heavy_check_mark:                                               | N/A                                                              |
| `messageStorageEnforceInTransit`                                 | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `projectId`                                                      | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `schemaEncoding`                                                 | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `schemaFirstRevisionId`                                          | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `schemaLastRevisionId`                                           | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `schemaName`                                                     | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [models.QueueHeartbeatStatus](../models/queueheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `subscriptionAckDeadlineSeconds`                                 | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionDeadLetterMaxDeliveryAttempts`                      | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionDeadLetterTopic`                                    | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionDetached`                                           | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionEnableMessageOrdering`                              | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionFilter`                                             | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionFullName`                                           | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionLabels`                                             | Record<string, *string*>                                         | :heavy_check_mark:                                               | N/A                                                              |
| `subscriptionMessageRetentionDuration`                           | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionName`                                               | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionPushAttributes`                                     | Record<string, *string*>                                         | :heavy_check_mark:                                               | N/A                                                              |
| `subscriptionPushConfigPresent`                                  | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionPushEndpoint`                                       | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionPushNoWrapperWriteMetadata`                         | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionPushOidcAudience`                                   | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionPushOidcServiceAccountEmail`                        | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionPushPubsubWrapperWriteMetadata`                     | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionRetainAckedMessages`                                | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `subscriptionState`                                              | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `topicFullName`                                                  | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `topicLabels`                                                    | Record<string, *string*>                                         | :heavy_check_mark:                                               | N/A                                                              |
| `topicMessageRetentionDuration`                                  | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `topicName`                                                      | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `topicState`                                                     | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `backend`                                                        | *"gcpPubSub"*                                                    | :heavy_check_mark:                                               | N/A                                                              |