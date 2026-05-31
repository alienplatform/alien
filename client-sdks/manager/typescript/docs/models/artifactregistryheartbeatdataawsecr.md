# ArtifactRegistryHeartbeatDataAwsEcr

## Example Usage

```typescript
import { ArtifactRegistryHeartbeatDataAwsEcr } from "@alienplatform/manager-api/models";

let value: ArtifactRegistryHeartbeatDataAwsEcr = {
  region: "<value>",
  registryId: "<id>",
  registryUri: "https://unlined-bowler.com/",
  repositories: [
    {
      createdAt: 781.7,
      kmsKeyPresent: true,
      registryId: "<id>",
      repositoryArn: "<value>",
      repositoryName: "<value>",
      repositoryUri: "https://wavy-eggplant.org",
    },
  ],
  repositoriesTruncated: true,
  repositoryCount: 952265,
  repositoryPrefix: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "awsEcr",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
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