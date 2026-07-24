# PersistImportedDeploymentRequestPendingPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value:
  PersistImportedDeploymentRequestPendingPreparedStackOverrideGcpStack = {
    scope: "<value>",
  };
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `condition`                                                                              | *models.PersistImportedDeploymentRequestPendingPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `scope`                                                                                  | *string*                                                                                 | :heavy_check_mark:                                                                       | Scope (project/resource level)                                                           |
