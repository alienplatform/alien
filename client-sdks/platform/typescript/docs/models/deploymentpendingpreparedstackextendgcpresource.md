# DeploymentPendingPreparedStackExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPendingPreparedStackExtendGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `condition`                                                         | *models.DeploymentPendingPreparedStackExtendResourceConditionUnion* | :heavy_minus_sign:                                                  | N/A                                                                 |
| `scope`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Scope (project/resource level)                                      |
