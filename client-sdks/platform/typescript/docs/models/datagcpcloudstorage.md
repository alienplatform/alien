# DataGcpCloudStorage

## Example Usage

```typescript
import { DataGcpCloudStorage } from "@alienplatform/platform-api/models";

let value: DataGcpCloudStorage = {
  encryptionConfigPresent: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-04-25T23:42:18.878Z"),
      severity: "error",
    },
  ],
  lifecyclePresent: false,
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "failed",
    partial: true,
    stale: true,
  },
  backend: "gcpCloudStorage",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `bucketId`                                                                     | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `defaultKmsKeyName`                                                            | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `encryptionConfigPresent`                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent2](../models/syncreconcilerequestevent2.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecyclePresent`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycleRuleCount`                                                           | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `location`                                                                     | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `locationType`                                                                 | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `publicAccessPrevention`                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `retentionPeriod`                                                              | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `retentionPolicyEffectiveTime`                                                 | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `retentionPolicyIsLocked`                                                      | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `softDeleteEffectiveTime`                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `softDeleteRetentionDurationSeconds`                                           | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus2](../models/heartbeatstatus2.md)                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `storageClass`                                                                 | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `uniformBucketLevelAccessEnabled`                                              | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `uniformBucketLevelAccessLockedTime`                                           | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `versioningEnabled`                                                            | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `backend`                                                                      | *"gcpCloudStorage"*                                                            | :heavy_check_mark:                                                             | N/A                                                                            |