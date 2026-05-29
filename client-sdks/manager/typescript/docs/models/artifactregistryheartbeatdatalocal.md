# ArtifactRegistryHeartbeatDataLocal

## Example Usage

```typescript
import { ArtifactRegistryHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: ArtifactRegistryHeartbeatDataLocal = {
  events: [],
  reachable: false,
  registryUrl: "https://excited-armoire.net",
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
  backend: "local",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `events`                                                                               | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                 | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `reachable`                                                                            | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `registryUrl`                                                                          | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `status`                                                                               | [models.ArtifactRegistryHeartbeatStatus](../models/artifactregistryheartbeatstatus.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `backend`                                                                              | *"local"*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |