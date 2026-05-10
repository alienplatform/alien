# PersistImportedDeploymentRequestProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestProfileGcpStack } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `condition`                                                         | *models.PersistImportedDeploymentRequestProfileStackConditionUnion* | :heavy_minus_sign:                                                  | N/A                                                                 |
| `scope`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Scope (project/resource level)                                      |