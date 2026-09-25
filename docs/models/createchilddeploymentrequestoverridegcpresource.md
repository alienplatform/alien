# CreateChildDeploymentRequestOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CreateChildDeploymentRequestOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `condition`                                                         | *models.CreateChildDeploymentRequestOverrideResourceConditionUnion* | :heavy_minus_sign:                                                  | N/A                                                                 |
| `scope`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Scope (project/resource level)                                      |