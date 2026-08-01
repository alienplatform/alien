# PersistImportedDeploymentRequestExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExtendGcpResource } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `condition`                                                           | *models.PersistImportedDeploymentRequestExtendResourceConditionUnion* | :heavy_minus_sign:                                                    | N/A                                                                   |
| `scope`                                                               | *string*                                                              | :heavy_check_mark:                                                    | Scope (project/resource level)                                        |