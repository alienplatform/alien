# DeploymentDetailResponsePendingPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `condition`                                                                      | *models.DeploymentDetailResponsePendingPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                                               | N/A                                                                              |
| `scope`                                                                          | *string*                                                                         | :heavy_check_mark:                                                               | Scope (project/resource level)                                                   |
