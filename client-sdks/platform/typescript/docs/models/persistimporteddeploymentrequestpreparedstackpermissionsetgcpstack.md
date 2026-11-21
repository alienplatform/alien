# PersistImportedDeploymentRequestPreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackPermissionSetGcpStack =
  {
    scope: "<value>",
  };
```

## Fields

| Field                                                                                     | Type                                                                                      | Required                                                                                  | Description                                                                               |
| ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `condition`                                                                               | *models.PersistImportedDeploymentRequestPreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                                        | N/A                                                                                       |
| `scope`                                                                                   | *string*                                                                                  | :heavy_check_mark:                                                                        | Scope (project/resource level)                                                            |