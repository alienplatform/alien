# StorageHeartbeatDataAwsS3

## Example Usage

```typescript
import { StorageHeartbeatDataAwsS3 } from "@alienplatform/manager-api/models";

let value: StorageHeartbeatDataAwsS3 = {
  encryptionConfigPresent: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  lifecyclePresent: false,
  name: "<value>",
  publicAccessBlockPresent: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "awsS3",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `blockPublicAcls`                                                    | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `blockPublicPolicy`                                                  | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `bucketAclPresent`                                                   | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `bucketLocation`                                                     | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `bucketPolicyPresent`                                                | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `encryptionConfigPresent`                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `encryptionEnabled`                                                  | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `events`                                                             | [models.HeartbeatEvent](../models/heartbeatevent.md)[]               | :heavy_check_mark:                                                   | N/A                                                                  |
| `ignorePublicAcls`                                                   | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `lifecyclePresent`                                                   | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycleRuleCount`                                                 | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `publicAccessBlockPresent`                                           | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `region`                                                             | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `restrictPublicBuckets`                                              | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [models.StorageHeartbeatStatus](../models/storageheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `versioningEnabled`                                                  | *boolean*                                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `versioningStatus`                                                   | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `backend`                                                            | *"awsS3"*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |