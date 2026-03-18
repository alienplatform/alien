# DeploymentOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `condition`                                       | *models.DeploymentOverrideResourceConditionUnion* | :heavy_minus_sign:                                | N/A                                               |
| `scope`                                           | *string*                                          | :heavy_check_mark:                                | Scope (project/resource level)                    |