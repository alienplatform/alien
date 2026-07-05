# DataBuild

## Example Usage

```typescript
import { DataBuild } from "@alienplatform/platform-api/models";

let value: DataBuild = {
  data: {
    encryptionKeyPresent: false,
    environmentVariableCount: 19119,
    projectName: "<value>",
    serviceRolePresent: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "failed",
      partial: false,
      stale: true,
    },
    backend: "awsCodeBuild",
  },
  resourceType: "build",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion14* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"build"*                                | :heavy_check_mark:                       | N/A                                      |