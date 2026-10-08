# OperationPermission

Canonical Kubernetes permission snapshot for one enabled operation.

## Example Usage

```typescript
import { OperationPermission } from "@alienplatform/platform-api/models";

let value: OperationPermission = {
  operation: "<value>",
  permissions: {
    rules: [
      {
        apiGroup: "<value>",
        reason: "<value>",
        resource: "<value>",
        verbs: [
          "<value 1>",
          "<value 2>",
          "<value 3>",
        ],
      },
    ],
    schemaVersion: 347311,
  },
  plugin: "<value>",
  tier: "mutating",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `operation`                                                             | *string*                                                                | :heavy_check_mark:                                                      | Operation name within the plugin.                                       |
| `permissions`                                                           | [models.PackagePermissions](../models/packagepermissions.md)            | :heavy_check_mark:                                                      | Versioned Kubernetes API requirements declared by an enabled operation. |
| `plugin`                                                                | *string*                                                                | :heavy_check_mark:                                                      | Operations plugin name.                                                 |
| `tier`                                                                  | [models.PackageTier](../models/packagetier.md)                          | :heavy_check_mark:                                                      | Effective risk tier of an enabled operation in a Helm build snapshot.   |