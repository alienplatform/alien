# DeploymentExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentExtendGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `condition`                                     | *models.DeploymentExtendResourceConditionUnion* | :heavy_minus_sign:                              | N/A                                             |
| `scope`                                         | *string*                                        | :heavy_check_mark:                              | Scope (project/resource level)                  |