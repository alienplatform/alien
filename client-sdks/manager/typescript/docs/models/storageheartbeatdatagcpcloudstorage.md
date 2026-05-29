# StorageHeartbeatDataGcpCloudStorage

## Example Usage

```typescript
import { StorageHeartbeatDataGcpCloudStorage } from "@alienplatform/manager-api/models";

let value: StorageHeartbeatDataGcpCloudStorage = {
  encryptionConfigPresent: false,
  lifecyclePresent: false,
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "gcpCloudStorage",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `bucketId`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `defaultKmsKeyName`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `encryptionConfigPresent`                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecyclePresent`                                                   | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycleRuleCount`                                                 | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `location`                                                           | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `locationType`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `publicAccessPrevention`                                             | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `retentionPeriod`                                                    | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `retentionPolicyEffectiveTime`                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `retentionPolicyIsLocked`                                            | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `softDeleteEffectiveTime`                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `softDeleteRetentionDurationSeconds`                                 | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [models.StorageHeartbeatStatus](../models/storageheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `storageClass`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `uniformBucketLevelAccessEnabled`                                    | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `uniformBucketLevelAccessLockedTime`                                 | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `versioningEnabled`                                                  | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `backend`                                                            | *"gcpCloudStorage"*                                                  | :heavy_check_mark:                                                   | N/A                                                                  |