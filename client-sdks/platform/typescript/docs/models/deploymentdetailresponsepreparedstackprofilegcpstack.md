# DeploymentDetailResponsePreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `condition`                                                                 | *models.DeploymentDetailResponsePreparedStackProfileGcpStackConditionUnion* | :heavy_minus_sign:                                                          | N/A                                                                         |
| `scope`                                                                     | *string*                                                                    | :heavy_check_mark:                                                          | Scope (project/resource level)                                              |