# DataBuild

## Example Usage

```typescript
import { DataBuild } from "@alienplatform/platform-api/models";

let value: DataBuild = {
  data: {
    encryptionKeyPresent: false,
    environmentVariableCount: 19119,
    events: [],
    projectName: "<value>",
    serviceRolePresent: false,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
    backend: "awsCodeBuild",
  },
  resourceType: "build",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion13* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"build"*                                | :heavy_check_mark:                       | N/A                                      |