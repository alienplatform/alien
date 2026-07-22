# DeploymentPendingPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPendingPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `condition`                                                        | *models.DeploymentPendingPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                                 | N/A                                                                |
| `scope`                                                            | *string*                                                           | :heavy_check_mark:                                                 | Scope (project/resource level)                                     |
