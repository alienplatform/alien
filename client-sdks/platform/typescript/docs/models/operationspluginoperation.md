# OperationsPluginOperation

## Example Usage

```typescript
import { OperationsPluginOperation } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperation = {
  name: "<value>",
  tier: "mutating",
  description: "reopen brown likewise how likewise of vicinity nectarine yahoo",
  inputSchema: {
    "key": "<value>",
    "key1": "<value>",
  },
  outputSchema: {},
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | Operation name, unique within the plugin.                                          |
| `tier`                                                                             | [models.OperationsPluginOperationTier](../models/operationspluginoperationtier.md) | :heavy_check_mark:                                                                 | Effective risk tier for this operation.                                            |
| `description`                                                                      | *string*                                                                           | :heavy_check_mark:                                                                 | Human-readable description.                                                        |
| `inputSchema`                                                                      | Record<string, *any*>                                                              | :heavy_check_mark:                                                                 | JSON Schema for operation parameters when the plugin publishes one.                |
| `outputSchema`                                                                     | Record<string, *any*>                                                              | :heavy_check_mark:                                                                 | JSON Schema for a successful result when the plugin publishes one.                 |