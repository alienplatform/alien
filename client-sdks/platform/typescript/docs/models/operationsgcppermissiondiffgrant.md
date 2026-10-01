# OperationsGcpPermissionDiffGrant

## Example Usage

```typescript
import { OperationsGcpPermissionDiffGrant } from "@alienplatform/platform-api/models";

let value: OperationsGcpPermissionDiffGrant = {
  permission: "<value>",
  scope: "projects/${projectName}",
  sources: [
    {
      plugin: "<value>",
      operation: "<value>",
      reason: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `permission`                                                                                       | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `scope`                                                                                            | [models.OperationsGcpPermissionDiffGrantScope](../models/operationsgcppermissiondiffgrantscope.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `sources`                                                                                          | [models.OperationsPermissionDiffSource](../models/operationspermissiondiffsource.md)[]             | :heavy_check_mark:                                                                                 | N/A                                                                                                |