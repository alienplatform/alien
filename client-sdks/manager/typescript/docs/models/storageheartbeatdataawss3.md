# StorageHeartbeatDataAwsS3

## Example Usage

```typescript
import { StorageHeartbeatDataAwsS3 } from "@alienplatform/manager-api/models";

let value: StorageHeartbeatDataAwsS3 = {
  encryptionConfigPresent: true,
  lifecyclePresent: false,
  name: "<value>",
  publicAccessBlockPresent: false,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
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