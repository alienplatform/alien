# DataAwsEcr

## Example Usage

```typescript
import { DataAwsEcr } from "@alienplatform/platform-api/models/operations";

let value: DataAwsEcr = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-07-20T04:34:55.254Z"),
      severity: "info",
    },
  ],
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://shiny-shadowbox.info/",
  repositories: [
    {
      createdAt: 3232.31,
      kmsKeyPresent: true,
      registryId: "<id>",
      repositoryArn: "<value>",
      repositoryName: "<value>",
      repositoryUri: "https://back-wear.com/",
    },
  ],
  repositoriesTruncated: true,
  repositoryCount: 601812,
  repositoryPrefix: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  backend: "awsEcr",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent46](../../models/operations/getrawresourceheartbeatevent46.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `pullRoleArn`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `pushRoleArn`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `region`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `registryId`                                                                                             | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `registryUri`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `repositories`                                                                                           | [operations.Repository](../../models/operations/repository.md)[]                                         | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `repositoriesTruncated`                                                                                  | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `repositoryCount`                                                                                        | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `repositoryPrefix`                                                                                       | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus46](../../models/operations/datastatus46.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"awsEcr"*                                                                                               | :heavy_check_mark:                                                                                       | N/A                                                                                                      |