# DeploymentDetailResponsePendingPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `condition`                                                                     | *models.DeploymentDetailResponsePendingPreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                              | N/A                                                                             |
| `scope`                                                                         | *string*                                                                        | :heavy_check_mark:                                                              | Scope (project/resource level)                                                  |
