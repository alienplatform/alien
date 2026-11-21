# PersistImportedDeploymentRequestPreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value:
  PersistImportedDeploymentRequestPreparedStackPermissionSetGcpResource = {
    scope: "<value>",
  };
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `condition`                                                                                  | *models.PersistImportedDeploymentRequestPreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `scope`                                                                                      | *string*                                                                                     | :heavy_check_mark:                                                                           | Scope (project/resource level)                                                               |