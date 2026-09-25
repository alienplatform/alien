# CreateChildDeploymentRequestProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { CreateChildDeploymentRequestProfileGcpStack } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `condition`                                                | *models.CreateChildDeploymentRequestProfileConditionUnion* | :heavy_minus_sign:                                         | N/A                                                        |
| `scope`                                                    | *string*                                                   | :heavy_check_mark:                                         | Scope (project/resource level)                             |