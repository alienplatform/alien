# PreparedDeploymentStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PreparedDeploymentStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackPermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `condition`                                                            | *models.PreparedDeploymentStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                     | N/A                                                                    |
| `scope`                                                                | *string*                                                               | :heavy_check_mark:                                                     | Scope (project/resource level)                                         |