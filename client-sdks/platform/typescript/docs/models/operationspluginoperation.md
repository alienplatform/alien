# OperationsPluginOperation

## Example Usage

```typescript
import { OperationsPluginOperation } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperation = {
  name: "<value>",
  tier: "mutating",
  description: "reopen brown likewise how likewise of vicinity nectarine yahoo",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | Operation name, unique within the plugin.                                          |
| `tier`                                                                             | [models.OperationsPluginOperationTier](../models/operationspluginoperationtier.md) | :heavy_check_mark:                                                                 | Effective risk tier for this operation.                                            |
| `description`                                                                      | *string*                                                                           | :heavy_check_mark:                                                                 | Human-readable description.                                                        |