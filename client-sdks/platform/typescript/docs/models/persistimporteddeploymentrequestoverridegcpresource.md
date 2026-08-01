# PersistImportedDeploymentRequestOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `condition`                                                             | *models.PersistImportedDeploymentRequestOverrideResourceConditionUnion* | :heavy_minus_sign:                                                      | N/A                                                                     |
| `scope`                                                                 | *string*                                                                | :heavy_check_mark:                                                      | Scope (project/resource level)                                          |