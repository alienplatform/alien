# ResourceHeartbeatDataArtifactRegistry

## Example Usage

```typescript
import { ResourceHeartbeatDataArtifactRegistry } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataArtifactRegistry = {
  data: {
    events: [],
    region: "<value>",
    registryId: "<id>",
    registryUri: "https://orange-halt.biz/",
    repositories: [
      {
        createdAt: 2948.54,
        kmsKeyPresent: true,
        registryId: "<id>",
        repositoryArn: "<value>",
        repositoryName: "<value>",
        repositoryUri: "https://impolite-bran.name/",
      },
    ],
    repositoriesTruncated: false,
    repositoryCount: 16580,
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
  },
  resourceType: "artifact-registry",
};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `data`                                 | *models.ArtifactRegistryHeartbeatData* | :heavy_check_mark:                     | N/A                                    |
| `resourceType`                         | *"artifact-registry"*                  | :heavy_check_mark:                     | N/A                                    |