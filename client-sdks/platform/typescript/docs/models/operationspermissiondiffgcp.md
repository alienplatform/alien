# OperationsPermissionDiffGcp

## Example Usage

```typescript
import { OperationsPermissionDiffGcp } from "@alienplatform/platform-api/models";

let value: OperationsPermissionDiffGcp = {
  added: [
    {
      permission: "<value>",
      scope: "projects/${projectName}/buckets/${resourceName}",
      sources: [],
    },
  ],
  removed: [],
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `added`                                                                                    | [models.OperationsGcpPermissionDiffGrant](../models/operationsgcppermissiondiffgrant.md)[] | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `removed`                                                                                  | [models.OperationsGcpPermissionDiffGrant](../models/operationsgcppermissiondiffgrant.md)[] | :heavy_check_mark:                                                                         | N/A                                                                                        |
