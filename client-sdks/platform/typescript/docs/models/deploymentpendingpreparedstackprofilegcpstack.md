# DeploymentPendingPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPendingPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `condition`                                                       | *models.DeploymentPendingPreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                | N/A                                                               |
| `scope`                                                           | *string*                                                          | :heavy_check_mark:                                                | Scope (project/resource level)                                    |
