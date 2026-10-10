# OperationsPlugin

## Example Usage

```typescript
import { OperationsPlugin } from "@alienplatform/platform-api/models";

let value: OperationsPlugin = {
  name: "<value>",
  version: "<value>",
  tier: "mutating",
  builtin: true,
  operations: [],
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `name`                                                                       | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `version`                                                                    | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `tier`                                                                       | [models.OperationsPluginTier](../models/operationsplugintier.md)             | :heavy_check_mark:                                                           | Plugin-level default tier.                                                   |
| `builtin`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | True for compiled-in plugins, false for custom bundles.                      |
| `operations`                                                                 | [models.OperationsPluginOperation](../models/operationspluginoperation.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `enabled`                                                                    | *boolean*                                                                    | :heavy_minus_sign:                                                           | N/A                                                                          |