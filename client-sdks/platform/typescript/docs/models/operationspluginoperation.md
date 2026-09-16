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
  timeoutSeconds: 463935,
  retries: {
    maxAttempts: 804051,
    intervalSeconds: 912712,
  },
  verification: {
    changes: "<value>",
    pollOperation: "<value>",
    pollParamsFromResult: {
      "key": "<value>",
    },
    successField: "<value>",
    successValue: "<value>",
    timeoutSeconds: 61821,
  },
  sensitiveOutput: {
    kind: "redact",
    fields: [
      "<value 1>",
    ],
  },
  requiredPermissions: [
    "<value 1>",
    "<value 2>",
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
        actions: [],
        resources: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
        condition: {
          "key": {},
        },
        reason: "<value>",
      },
    ],
    gcp: [],
  },
  kubernetesPermissions: {
    schemaVersion: 2286.04,
    rules: [],
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                               | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Operation name, unique within the plugin.                                                                            |
| `tier`                                                                                                               | [models.OperationsPluginOperationTier](../models/operationspluginoperationtier.md)                                   | :heavy_check_mark:                                                                                                   | Effective risk tier for this operation.                                                                              |
| `description`                                                                                                        | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Human-readable description.                                                                                          |
| `inputSchema`                                                                                                        | Record<string, *any*>                                                                                                | :heavy_check_mark:                                                                                                   | JSON Schema for operation parameters when the plugin publishes one.                                                  |
| `outputSchema`                                                                                                       | Record<string, *any*>                                                                                                | :heavy_check_mark:                                                                                                   | JSON Schema for a successful result when the plugin publishes one.                                                   |
| `timeoutSeconds`                                                                                                     | *number*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `retries`                                                                                                            | [models.OperationsPluginOperationRetries](../models/operationspluginoperationretries.md)                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `verification`                                                                                                       | [models.OperationsPluginOperationVerification](../models/operationspluginoperationverification.md)                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `sensitiveOutput`                                                                                                    | *models.OperationsPluginOperationSensitiveOutputUnion*                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `requiredPermissions`                                                                                                | *string*[]                                                                                                           | :heavy_check_mark:                                                                                                   | IDs of permission sets (see alien-permissions) this operation requires. Empty when the operation declares none.      |
| `permissions`                                                                                                        | [models.OperationsPluginOperationPermissions](../models/operationspluginoperationpermissions.md)                     | :heavy_check_mark:                                                                                                   | Cloud permissions required to execute this operation.                                                                |
| `kubernetesPermissions`                                                                                              | [models.OperationsPluginOperationKubernetesPermissions](../models/operationspluginoperationkubernetespermissions.md) | :heavy_check_mark:                                                                                                   | Kubernetes RBAC required to execute this operation. Null when the operation declares none.                           |