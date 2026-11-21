# DeploymentDetailResponsePreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `condition`                                                                          | *models.DeploymentDetailResponsePreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `scope`                                                                              | *string*                                                                             | :heavy_check_mark:                                                                   | Scope (project/resource level)                                                       |