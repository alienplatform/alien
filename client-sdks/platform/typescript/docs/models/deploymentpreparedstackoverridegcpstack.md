# DeploymentPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `condition`                                                 | *models.DeploymentPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                          | N/A                                                         |
| `scope`                                                     | *string*                                                    | :heavy_check_mark:                                          | Scope (project/resource level)                              |
