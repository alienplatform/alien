# CreateChildDeploymentRequestExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CreateChildDeploymentRequestExtendGcpResource } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `condition`                                                       | *models.CreateChildDeploymentRequestExtendResourceConditionUnion* | :heavy_minus_sign:                                                | N/A                                                               |
| `scope`                                                           | *string*                                                          | :heavy_check_mark:                                                | Scope (project/resource level)                                    |