# OperationsPluginOperationKubernetesPermissions

Kubernetes RBAC required to execute this operation. Null when the operation declares none.

## Example Usage

```typescript
import { OperationsPluginOperationKubernetesPermissions } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperationKubernetesPermissions = {
  schemaVersion: 7898.42,
  rules: [],
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `schemaVersion`                                                                      | *number*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `rules`                                                                              | [models.OperationsPluginOperationRule](../models/operationspluginoperationrule.md)[] | :heavy_check_mark:                                                                   | N/A                                                                                  |