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
  requiredPermissions: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  permissions: {
    azure: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    aws: [
      {
        effect: "Deny",
        actions: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
        resources: [],
        condition: {},
        reason: "<value>",
      },
    ],
    gcp: [
      {
        permissions: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
        scope: "projects/${projectName}",
        reason: "<value>",
      },
    ],
  },
};
```

## Fields

| Field                                                                                                           | Type                                                                                                            | Required                                                                                                        | Description                                                                                                     |
| --------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                          | *string*                                                                                                        | :heavy_check_mark:                                                                                              | Operation name, unique within the plugin.                                                                       |
| `tier`                                                                                                          | [models.OperationsPluginOperationTier](../models/operationspluginoperationtier.md)                              | :heavy_check_mark:                                                                                              | Effective risk tier for this operation.                                                                         |
| `description`                                                                                                   | *string*                                                                                                        | :heavy_check_mark:                                                                                              | Human-readable description.                                                                                     |
| `inputSchema`                                                                                                   | Record<string, *any*>                                                                                           | :heavy_check_mark:                                                                                              | JSON Schema for operation parameters when the plugin publishes one.                                             |
| `outputSchema`                                                                                                  | Record<string, *any*>                                                                                           | :heavy_check_mark:                                                                                              | JSON Schema for a successful result when the plugin publishes one.                                              |
| `requiredPermissions`                                                                                           | *string*[]                                                                                                      | :heavy_check_mark:                                                                                              | IDs of permission sets (see alien-permissions) this operation requires. Empty when the operation declares none. |
| `permissions`                                                                                                   | [models.OperationsPluginOperationPermissions](../models/operationspluginoperationpermissions.md)                | :heavy_check_mark:                                                                                              | Cloud permissions required to execute this operation.                                                           |