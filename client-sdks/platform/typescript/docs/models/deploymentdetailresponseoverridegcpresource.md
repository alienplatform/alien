# DeploymentDetailResponseOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponseOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.DeploymentDetailResponseOverrideResourceConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |