# PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value:
  PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpResource =
    {
      scope: "<value>",
    };
```

## Fields

| Field                                                                                               | Type                                                                                                | Required                                                                                            | Description                                                                                         |
| --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `condition`                                                                                         | *models.PersistImportedDeploymentRequestPendingPreparedStackPermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                                                                  | N/A                                                                                                 |
| `scope`                                                                                             | *string*                                                                                            | :heavy_check_mark:                                                                                  | Scope (project/resource level)                                                                      |