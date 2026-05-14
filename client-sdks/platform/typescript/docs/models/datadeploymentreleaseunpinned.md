# DataDeploymentReleaseUnpinned

## Example Usage

```typescript
import { DataDeploymentReleaseUnpinned } from "@alienplatform/platform-api/models";

let value: DataDeploymentReleaseUnpinned = {
  deploymentId: "<id>",
  previousPinnedReleaseId: "<id>",
  type: "DeploymentReleaseUnpinned",
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `deploymentId`                               | *string*                                     | :heavy_check_mark:                           | ID of the deployment                         |
| `previousPinnedReleaseId`                    | *string*                                     | :heavy_check_mark:                           | ID of the release that was previously pinned |
| `type`                                       | *"DeploymentReleaseUnpinned"*                | :heavy_check_mark:                           | N/A                                          |