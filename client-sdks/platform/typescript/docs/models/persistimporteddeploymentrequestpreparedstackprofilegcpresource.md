# PersistImportedDeploymentRequestPreparedStackProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackProfileGcpResource } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                               | Type                                                                                | Required                                                                            | Description                                                                         |
| ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `condition`                                                                         | *models.PersistImportedDeploymentRequestPreparedStackProfileResourceConditionUnion* | :heavy_minus_sign:                                                                  | N/A                                                                                 |
| `scope`                                                                             | *string*                                                                            | :heavy_check_mark:                                                                  | Scope (project/resource level)                                                      |
