# ArtifactRegistryHeartbeatDataLocal

## Example Usage

```typescript
import { ArtifactRegistryHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: ArtifactRegistryHeartbeatDataLocal = {
  reachable: true,
  registryUrl: "https://which-devastation.com",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `reachable`                                                                            | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `registryUrl`                                                                          | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `status`                                                                               | [models.ArtifactRegistryHeartbeatStatus](../models/artifactregistryheartbeatstatus.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `backend`                                                                              | *"local"*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |