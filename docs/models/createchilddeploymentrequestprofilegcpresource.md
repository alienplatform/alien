# CreateChildDeploymentRequestProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CreateChildDeploymentRequestProfileGcpResource } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `condition`                                                        | *models.CreateChildDeploymentRequestProfileResourceConditionUnion* | :heavy_minus_sign:                                                 | N/A                                                                |
| `scope`                                                            | *string*                                                           | :heavy_check_mark:                                                 | Scope (project/resource level)                                     |