# CreateChildDeploymentRequestExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { CreateChildDeploymentRequestExtendGcpStack } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `condition`                                               | *models.CreateChildDeploymentRequestExtendConditionUnion* | :heavy_minus_sign:                                        | N/A                                                       |
| `scope`                                                   | *string*                                                  | :heavy_check_mark:                                        | Scope (project/resource level)                            |