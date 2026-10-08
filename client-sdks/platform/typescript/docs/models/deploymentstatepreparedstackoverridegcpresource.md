# DeploymentStatePreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `condition`                                                         | *models.DeploymentStatePreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                                  | N/A                                                                 |
| `scope`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Scope (project/resource level)                                      |