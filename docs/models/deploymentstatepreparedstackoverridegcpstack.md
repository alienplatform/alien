# DeploymentStatePreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `condition`                                                      | *models.DeploymentStatePreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                               | N/A                                                              |
| `scope`                                                          | *string*                                                         | :heavy_check_mark:                                               | Scope (project/resource level)                                   |