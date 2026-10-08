# DeploymentPendingPreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPendingPreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `condition`                                                           | *models.DeploymentPendingPreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                                    | N/A                                                                   |
| `scope`                                                               | *string*                                                              | :heavy_check_mark:                                                    | Scope (project/resource level)                                        |
