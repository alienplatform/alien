# PersistImportedDeploymentRequestExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExtendGcpStack } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `condition`                                                        | *models.PersistImportedDeploymentRequestExtendStackConditionUnion* | :heavy_minus_sign:                                                 | N/A                                                                |
| `scope`                                                            | *string*                                                           | :heavy_check_mark:                                                 | Scope (project/resource level)                                     |