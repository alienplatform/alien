# DataAwsS3

## Example Usage

```typescript
import { DataAwsS3 } from "@alienplatform/platform-api/models/operations";

let value: DataAwsS3 = {
  encryptionConfigPresent: true,
  lifecyclePresent: false,
  name: "<value>",
  publicAccessBlockPresent: true,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "awsS3",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `blockPublicAcls`                                                | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `blockPublicPolicy`                                              | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `bucketAclPresent`                                               | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `bucketLocation`                                                 | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `bucketPolicyPresent`                                            | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `encryptionConfigPresent`                                        | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `encryptionEnabled`                                              | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `ignorePublicAcls`                                               | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `lifecyclePresent`                                               | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `lifecycleRuleCount`                                             | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `publicAccessBlockPresent`                                       | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `region`                                                         | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `restrictPublicBuckets`                                          | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [operations.DataStatus1](../../models/operations/datastatus1.md) | :heavy_check_mark:                                               | N/A                                                              |
| `versioningEnabled`                                              | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `versioningStatus`                                               | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `backend`                                                        | *"awsS3"*                                                        | :heavy_check_mark:                                               | N/A                                                              |