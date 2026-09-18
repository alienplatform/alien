# OperationsCatalogKubernetesPermissions

Kubernetes RBAC required to execute this operation. Omitted by older servers and null when the operation declares none.

## Example Usage

```typescript
import { OperationsCatalogKubernetesPermissions } from "@alienplatform/platform-api/models";

let value: OperationsCatalogKubernetesPermissions = {
  schemaVersion: 533.43,
  rules: [
    {
      apiGroup: "<value>",
      resource: "<value>",
      verbs: [],
      reason: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `schemaVersion`                                                                                                | *number*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `rules`                                                                                                        | [models.OperationsCatalogKubernetesPermissionsRule](../models/operationscatalogkubernetespermissionsrule.md)[] | :heavy_check_mark:                                                                                             | N/A                                                                                                            |