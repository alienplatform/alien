# GeneratedPermission

## Example Usage

```typescript
import { GeneratedPermission } from "@alienplatform/platform-api/models/operations";

let value: GeneratedPermission = {
  plugin: "<value>",
  pluginVersion: "<value>",
  operation: "<value>",
  tier: "read-only",
  cloud: {
    azure: [
      "<value 1>",
      "<value 2>",
    ],
    aws: [],
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

| Field                                                                                                                   | Type                                                                                                                    | Required                                                                                                                | Description                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `plugin`                                                                                                                | *string*                                                                                                                | :heavy_check_mark:                                                                                                      | N/A                                                                                                                     |
| `pluginVersion`                                                                                                         | *string*                                                                                                                | :heavy_check_mark:                                                                                                      | N/A                                                                                                                     |
| `operation`                                                                                                             | *string*                                                                                                                | :heavy_check_mark:                                                                                                      | N/A                                                                                                                     |
| `tier`                                                                                                                  | [operations.GeneratedPermissionTier](../../models/operations/generatedpermissiontier.md)                                | :heavy_check_mark:                                                                                                      | N/A                                                                                                                     |
| `cloud`                                                                                                                 | [operations.GetRemoteOperatorProjectSummaryCloud](../../models/operations/getremoteoperatorprojectsummarycloud.md)      | :heavy_check_mark:                                                                                                      | Cloud permissions required to execute this operation.                                                                   |
| `kubernetes`                                                                                                            | [models.OperationsCatalogKubernetesPermissions](../../models/operationscatalogkubernetespermissions.md)                 | :heavy_minus_sign:                                                                                                      | Kubernetes RBAC required to execute this operation. Omitted by older servers and null when the operation declares none. |