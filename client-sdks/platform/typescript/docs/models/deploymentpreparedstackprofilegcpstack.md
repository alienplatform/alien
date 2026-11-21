# DeploymentPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `condition`                                                   | *models.DeploymentPreparedStackProfileGcpStackConditionUnion* | :heavy_minus_sign:                                            | N/A                                                           |
| `scope`                                                       | *string*                                                      | :heavy_check_mark:                                            | Scope (project/resource level)                                |