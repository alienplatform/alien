# DeploymentListItemResponseEnvironmentInfoGcp

GCP-specific environment information

## Example Usage

```typescript
import { DeploymentListItemResponseEnvironmentInfoGcp } from "@aliendotdev/platform-api/models";

let value: DeploymentListItemResponseEnvironmentInfoGcp = {
  projectId: "<id>",
  projectNumber: "<value>",
  region: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `projectId`                                                                                        | *string*                                                                                           | :heavy_check_mark:                                                                                 | GCP project ID (e.g., "my-project")                                                                |
| `projectNumber`                                                                                    | *string*                                                                                           | :heavy_check_mark:                                                                                 | GCP project number (e.g., "123456789012")                                                          |
| `region`                                                                                           | *string*                                                                                           | :heavy_check_mark:                                                                                 | GCP region                                                                                         |
| `platform`                                                                                         | [models.DeploymentListItemResponsePlatformGcp](../models/deploymentlistitemresponseplatformgcp.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |