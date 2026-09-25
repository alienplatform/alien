# PublishChildDeploymentRequestProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PublishChildDeploymentRequestProfileGcpStack } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `condition`                                                 | *models.PublishChildDeploymentRequestProfileConditionUnion* | :heavy_minus_sign:                                          | N/A                                                         |
| `scope`                                                     | *string*                                                    | :heavy_check_mark:                                          | Scope (project/resource level)                              |