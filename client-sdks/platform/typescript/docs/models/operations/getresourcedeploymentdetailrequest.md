# GetResourceDeploymentDetailRequest

## Example Usage

```typescript
import { GetResourceDeploymentDetailRequest } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailRequest = {
  area: "daemon",
  deploymentId: "<id>",
  resourceId: "<id>",
  project: "<value>",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `area`                                                                                                   | [operations.GetResourceDeploymentDetailArea](../../models/operations/getresourcedeploymentdetailarea.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `deploymentId`                                                                                           | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `resourceId`                                                                                             | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `project`                                                                                                | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Filter by project ID or name.                                                                            |