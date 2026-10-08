# DeploymentStatePreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.DeploymentStatePreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |