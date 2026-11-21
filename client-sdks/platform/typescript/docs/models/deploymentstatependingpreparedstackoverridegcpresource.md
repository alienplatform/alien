# DeploymentStatePendingPreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `condition`                                                                | *models.DeploymentStatePendingPreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                                         | N/A                                                                        |
| `scope`                                                                    | *string*                                                                   | :heavy_check_mark:                                                         | Scope (project/resource level)                                             |