# PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value:
  PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpStack = {
    scope: "<value>",
  };
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `condition`                                                                                      | *models.PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpStackConditionUnion* | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `scope`                                                                                          | *string*                                                                                         | :heavy_check_mark:                                                                               | Scope (project/resource level)                                                                   |