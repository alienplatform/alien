# DeploymentPreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `condition`                                                            | *models.DeploymentPreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                     | N/A                                                                    |
| `scope`                                                                | *string*                                                               | :heavy_check_mark:                                                     | Scope (project/resource level)                                         |