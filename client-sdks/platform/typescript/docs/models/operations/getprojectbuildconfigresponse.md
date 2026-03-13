# GetProjectBuildConfigResponse

Build configuration retrieved successfully.

## Example Usage

```typescript
import { GetProjectBuildConfigResponse } from "@aliendotdev/platform-api/models/operations";

let value: GetProjectBuildConfigResponse = {
  managerUrl: "https://which-backburn.biz/",
  repositoryName: "<value>",
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `managerUrl`                         | *string*                             | :heavy_check_mark:                   | URL of the manager for this platform |
| `repositoryName`                     | *string*                             | :heavy_check_mark:                   | Name of the artifact repository      |
| `repositoryUri`                      | *string*                             | :heavy_minus_sign:                   | URI of the repository (if available) |