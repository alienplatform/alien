# DeploymentDetailResponsePreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackPermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `condition`                                                                       | *models.DeploymentDetailResponsePreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                                | N/A                                                                               |
| `scope`                                                                           | *string*                                                                          | :heavy_check_mark:                                                                | Scope (project/resource level)                                                    |