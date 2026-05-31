# DataAwsEcr

## Example Usage

```typescript
import { DataAwsEcr } from "@alienplatform/platform-api/models/operations";

let value: DataAwsEcr = {
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://mindless-bench.com",
  repositories: [
    {
      createdAt: 8040.15,
      kmsKeyPresent: true,
      registryId: "<id>",
      repositoryArn: "<value>",
      repositoryName: "<value>",
      repositoryUri: "https://glossy-disclosure.info",
    },
  ],
  repositoriesTruncated: true,
  repositoryCount: 980949,
  repositoryPrefix: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "awsEcr",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `pullRoleArn`                                                      | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `pushRoleArn`                                                      | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `region`                                                           | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `registryId`                                                       | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `registryUri`                                                      | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `repositories`                                                     | [operations.Repository](../../models/operations/repository.md)[]   | :heavy_check_mark:                                                 | N/A                                                                |
| `repositoriesTruncated`                                            | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `repositoryCount`                                                  | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `repositoryPrefix`                                                 | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus46](../../models/operations/datastatus46.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"awsEcr"*                                                         | :heavy_check_mark:                                                 | N/A                                                                |