# DataAwsS3

## Example Usage

```typescript
import { DataAwsS3 } from "@alienplatform/platform-api/models/operations";

let value: DataAwsS3 = {
  encryptionConfigPresent: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-11-04T18:46:41.433Z"),
      severity: "warning",
    },
  ],
  lifecyclePresent: true,
  name: "<value>",
  publicAccessBlockPresent: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  backend: "awsS3",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `blockPublicAcls`                                                                                      | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `blockPublicPolicy`                                                                                    | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `bucketAclPresent`                                                                                     | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `bucketLocation`                                                                                       | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `bucketPolicyPresent`                                                                                  | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `encryptionConfigPresent`                                                                              | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `encryptionEnabled`                                                                                    | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `events`                                                                                               | [operations.GetRawResourceHeartbeatEvent1](../../models/operations/getrawresourceheartbeatevent1.md)[] | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `ignorePublicAcls`                                                                                     | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `lifecyclePresent`                                                                                     | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `lifecycleRuleCount`                                                                                   | *number*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `name`                                                                                                 | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `publicAccessBlockPresent`                                                                             | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `region`                                                                                               | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `restrictPublicBuckets`                                                                                | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [operations.DataStatus1](../../models/operations/datastatus1.md)                                       | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `versioningEnabled`                                                                                    | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `versioningStatus`                                                                                     | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `backend`                                                                                              | *"awsS3"*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |