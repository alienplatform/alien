# DeploymentDetailResponsePreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `condition`                                                                  | *models.DeploymentDetailResponsePreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                                           | N/A                                                                          |
| `scope`                                                                      | *string*                                                                     | :heavy_check_mark:                                                           | Scope (project/resource level)                                               |
