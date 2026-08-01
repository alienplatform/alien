# DeploymentProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `condition`                                   | *models.DeploymentProfileStackConditionUnion* | :heavy_minus_sign:                            | N/A                                           |
| `scope`                                       | *string*                                      | :heavy_check_mark:                            | Scope (project/resource level)                |