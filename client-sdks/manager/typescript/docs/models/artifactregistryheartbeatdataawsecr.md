# ArtifactRegistryHeartbeatDataAwsEcr

## Example Usage

```typescript
import { ArtifactRegistryHeartbeatDataAwsEcr } from "@alienplatform/manager-api/models";

let value: ArtifactRegistryHeartbeatDataAwsEcr = {
  events: [],
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://burdensome-best-seller.net",
  repositories: [],
  repositoriesTruncated: true,
  repositoryCount: 717377,
  repositoryPrefix: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "awsEcr",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `events`                                                                               | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                 | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `pullRoleArn`                                                                          | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `pushRoleArn`                                                                          | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `region`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `registryId`                                                                           | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `registryUri`                                                                          | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `repositories`                                                                         | [models.AwsEcrRepositoryHeartbeatData](../models/awsecrrepositoryheartbeatdata.md)[]   | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `repositoriesTruncated`                                                                | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `repositoryCount`                                                                      | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `repositoryPrefix`                                                                     | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `status`                                                                               | [models.ArtifactRegistryHeartbeatStatus](../models/artifactregistryheartbeatstatus.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `backend`                                                                              | *"awsEcr"*                                                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |