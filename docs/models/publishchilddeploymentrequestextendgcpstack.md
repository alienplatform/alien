# PublishChildDeploymentRequestExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PublishChildDeploymentRequestExtendGcpStack } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `condition`                                                | *models.PublishChildDeploymentRequestExtendConditionUnion* | :heavy_minus_sign:                                         | N/A                                                        |
| `scope`                                                    | *string*                                                   | :heavy_check_mark:                                         | Scope (project/resource level)                             |