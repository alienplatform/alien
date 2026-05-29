# ResourceHeartbeatDataArtifactRegistry

## Example Usage

```typescript
import { ResourceHeartbeatDataArtifactRegistry } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataArtifactRegistry = {
  data: {
    region: "<value>",
    registryId: "<id>",
    registryUri: "https://dead-minor.info/",
    repositories: [],
    repositoriesTruncated: false,
    repositoryCount: 294854,
    repositoryPrefix: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "scaling",
      partial: true,
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