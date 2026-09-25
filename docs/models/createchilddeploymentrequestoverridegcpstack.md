# CreateChildDeploymentRequestOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { CreateChildDeploymentRequestOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `condition`                                                 | *models.CreateChildDeploymentRequestOverrideConditionUnion* | :heavy_minus_sign:                                          | N/A                                                         |
| `scope`                                                     | *string*                                                    | :heavy_check_mark:                                          | Scope (project/resource level)                              |