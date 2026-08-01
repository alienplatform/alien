# DeploymentDetailResponseExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponseExtendGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `condition`                                                | *models.DeploymentDetailResponseExtendStackConditionUnion* | :heavy_minus_sign:                                         | N/A                                                        |
| `scope`                                                    | *string*                                                   | :heavy_check_mark:                                         | Scope (project/resource level)                             |