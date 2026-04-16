# DeploymentDetailResponseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `condition`                                                 | *models.DeploymentDetailResponseProfileStackConditionUnion* | :heavy_minus_sign:                                          | N/A                                                         |
| `scope`                                                     | *string*                                                    | :heavy_check_mark:                                          | Scope (project/resource level)                              |