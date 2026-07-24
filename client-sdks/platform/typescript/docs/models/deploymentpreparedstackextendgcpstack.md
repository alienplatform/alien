# DeploymentPreparedStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `condition`                                               | *models.DeploymentPreparedStackExtendStackConditionUnion* | :heavy_minus_sign:                                        | N/A                                                       |
| `scope`                                                   | *string*                                                  | :heavy_check_mark:                                        | Scope (project/resource level)                            |
