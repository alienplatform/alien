# PublishChildDeploymentRequestExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PublishChildDeploymentRequestExtendGcpResource } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `condition`                                                        | *models.PublishChildDeploymentRequestExtendResourceConditionUnion* | :heavy_minus_sign:                                                 | N/A                                                                |
| `scope`                                                            | *string*                                                           | :heavy_check_mark:                                                 | Scope (project/resource level)                                     |