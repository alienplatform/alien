# DataBuild

## Example Usage

```typescript
import { DataBuild } from "@alienplatform/platform-api/models/operations";

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
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "unknown",
      partial: false,
      stale: true,
    },
    backend: "awsCodeBuild",
  },
  resourceType: "build",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *operations.DataUnion14* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"build"*                | :heavy_check_mark:       | N/A                      |